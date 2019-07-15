#[macro_use]
extern crate lalrpop_util;

pub mod ast;
mod indent_parser;

// why am I even using lalrpop for such a simple grammar
// f : (x1: A) -> (x2: B) -> (x3: C) -> D
// f x1 x2 x3 = a (b c (d e) f) g h
lalrpop_mod!(line_parser);

pub use indent_parser::ProgramParser;

pub fn type_check_all(programs: Vec<ast::Item>) {
    let mut global_names = Vec::with_capacity(programs.len());
    let mut globals = Vec::with_capacity(programs.len());
    for item in &programs {
        let (name, ty) = type_check_function(&global_names, &globals, item);
        println!("Success: {}", name);
        global_names.push(name);
        globals.push(ty);
    }
}

struct Item {
    ty: Expr,
    param_num: usize,
    def: Expr,
}

fn type_check_function(
    global_names: &Vec<String>,
    globals: &Vec<Item>,
    fun: &ast::Item,
) -> (String, Item) {
    for _ in &fun.associated {
        unimplemented!();
    }
    if fun.annotation.is_none() {
        if fun.definition.vars.len() > 0 {
            panic!("Terms with parameters must have a type annotation");
        } else {
            unimplemented!();
        }
    }
    let annotation = fun.annotation.as_ref().unwrap();
    if annotation.name != fun.definition.fname {
        panic!(
            "Annotation for {} was given alongside definition for {}",
            annotation.name,
            fun.definition.fname,
        );
    }
    let var_names = &fun.definition.vars;
    let param_num = var_names.len();
    // convert annotation
    let mut ty = convert_expr(
        global_names,
        &Default::default(),
        annotation.typ.clone()
    );
    eval(globals, &mut ty);
    // pull param types
    let mut result = ty.clone();
    let bindings: Vec<_> = result
        .arrow_params
        .drain(0..param_num)
        .collect();
    // convert definition
    let def = convert_expr(
        global_names,
        &Context::new(&var_names),
        fun.definition.body.clone(),
    );

    // check definition has the right type
    type_check_expr(globals, &Context::new(&bindings), &def, result);
    (fun.definition.fname.clone(), Item { ty, param_num, def })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ident {
    //Postulate(usize),
    Type, // Postulate(0)? Postulate(~0)?
    Global(usize),
    Local(usize),
}

#[derive(Clone, PartialEq, Debug)]
struct Expr {
    arrow_params: Vec<Expr>,
    head: Ident,
    tail: Vec<Expr>,
}

impl Expr {
    fn universe() -> Self {
        Expr {
            arrow_params: Vec::new(),
            head: Ident::Type,
            tail: Vec::new(),
        }
    }
    fn is_universe(self: &Self) -> bool {
        *self == Expr::universe()
    }
}

#[derive(Default)]
struct Context<'a, T> {
    prev_size: usize,
    prev: Option<&'a Context<'a, T>>,
    this: &'a [T],
}

impl<'a, T> Context<'a, T> {
    fn new(this: &'a [T]) -> Self {
        Context { this, prev_size: 0, prev: None }
    }
    fn push(self: &'a Self, next: &'a [T]) -> Self {
        // this doesn't actually shadow anything because of the size we use
        self.push_shadowed(next, self.size())
    }
    // shadows indeces, cannot be used with shadowed names
    // (maybe I should stop calling one of these shadowing...)
    fn push_shadowed(self: &'a Self, next: &'a [T], unshadowed: usize) -> Self {
        Context {
            prev_size: unshadowed,
            prev: Some(self),
            this: next,
        }
    }
    fn size(self: &Self) -> usize {
        self.prev_size + self.this.len()
    }

    // NOT valid on expressions with shadowed indeces
    // this is so that we can efficiently implement shadowed _parameter names_
    fn index_from_value(self: &Self, name: &T) -> Option<usize>
        where T: PartialEq,
    {
        let mut curr = self;
        loop {
            if let Some(result) = get_index(&curr.this, name) {
                return Some(result + curr.prev_size);
            }
            if let Some(prev) = &curr.prev {
                curr = prev;
            } else {
                return None;
            }
        }
    }
    fn value_from_index(self: &'a Self, index: usize) -> &'a T {
        let mut curr = self;
        loop {
            if index < curr.prev_size {
                curr = curr.prev.expect("Nonzero prev_size but no prev??");
            } else {
                return &curr.this[index - curr.prev_size];
            }
        }
    }
}

fn get_index<T: PartialEq>(names: &[T], name: &T) -> Option<usize> {
    let mut result = names.len();
    while result > 0 {
        result -= 1;
        if names[result] == *name {
            return Some(result);
        }
    }
    None
}

fn convert_expr(
    globals: &Vec<String>,
    locals: &Context<String>,
    mut expr: ast::Expr,
) -> Expr {
    let mut arrow_params = Vec::new();
    let mut new_locals = Vec::new();
    while let ast::Expr::Arrow(ast::ArrowExpr { params, output }) = expr {
        for (name, ty) in params {
            arrow_params.push(
                convert_expr(globals, &locals.push(&new_locals), ty)
            );
            new_locals.push(name.unwrap_or_else(|| "_".into()));
        }
        expr = *output;
    }
    let locals = locals.push(&new_locals);
    let alg = match expr {
        ast::Expr::Arrow(_) => unreachable!(),
        ast::Expr::Alg(alg) => alg,
    };

    let head = {
        if let Some(id) = locals.index_from_value(&alg.head) {
            Ident::Local(id)
        } else if let Some(id) = get_index(globals, &alg.head) {
            Ident::Global(id)
        } else if alg.head == "Type" {
            Ident::Type
        } else {
            panic!("Could not find term for identifier: {}", alg.head);
        }
    };
    let tail = alg
        .tail
        .into_iter()
        .map(|ex| convert_expr(globals, &locals, ex))
        .collect();
    Expr { arrow_params, head, tail }
}

fn type_check_expr(
    globals: &Vec<Item>,
    locals: &Context<Expr>,
    expr: &Expr,
    expected: Expr,
) {
    if expr.arrow_params.len() > 0 {
        // @Performance @Memory maybe Context<&Expr>??
        let mut new_locals = Vec::new();
        for each in &expr.arrow_params {
            type_check_expr(
                globals,
                &locals.push(&new_locals),
                each,
                Expr::universe(),
            );
            new_locals.push(each.clone());
        }
        // doesn't just check that the arrow expression was meant to be a type
        // the result expression also needs to be a type,
        // so we are implicitly assigning `expected = universe();`
        if !expected.is_universe() {
            panic!("Expected {:?}, got Type", expected);
        }
    }
    let mut checked = 0;
    let (mut actual_base, expr_ctx_size) = match expr.head {
        Ident::Local(i) => (locals.value_from_index(i).clone(), i),
        Ident::Global(i) => (globals[i].ty.clone(), 0),
        Ident::Type => {
            if expr.tail.len() > 0 {
                panic!("Cannot apply type to arguments");
            }
            return;
        },
    };
    while checked < expr.tail.len() {
        if actual_base.arrow_params.len() == 0 {
            eval(globals, &mut actual_base);
            if actual_base.arrow_params.len() == 0 {
                panic!("Cannot apply type family to argument(s): {:?}",
                       actual_base);
            }
        }
        // the first parameter of the actual type is
        // the expected type for the first argument
        let arg_expected_base = actual_base.arrow_params.remove(0);

        // @Memory maybe subst could take &mut param?
        // @Performance skip this cloning operation if i is 0?
        let arg_expected = subst(
            &arg_expected_base, expr_ctx_size,
            &expr.tail[0..checked], locals.size(),
        );
        type_check_expr(
            globals,
            locals,
            &expr.tail[checked],
            arg_expected,
        );
        checked += 1;
    }

    let mut actual = subst(
        &actual_base, expr_ctx_size,
        &expr.tail[0..checked], locals.size(),
    );
    eval(globals, &mut actual);
    if actual != expected {
        panic!("Types did not match\n\nexpected: {:?}\n\ngot: {:?}", expected, actual);
    }
}

fn eval_on(globals: &Vec<Item>, xs: &mut Vec<Expr>) {
    for x in xs {
        eval(globals, x);
    }
}

fn eval(globals: &Vec<Item>, expr: &mut Expr) {
    eval_on(globals, &mut expr.arrow_params);
    eval_on(globals, &mut expr.tail);

    while let Ident::Global(i) = expr.head {
        let param_num = globals[i].param_num;
        if expr.tail.len() >= param_num {
            let mut result = subst(
                &globals[i].def, 0,
                &expr.tail[0..param_num], 0,
            );
            // recurse... often redundant... @Performance? combine with subst?
            eval_on(globals, &mut result.arrow_params);
            eval_on(globals, &mut result.tail);
            expr.arrow_params.append(&mut result.arrow_params);
            expr.head = result.head;
            // @Performance we are allocating again every time...
            // could just combine these steps or something more tricky
            expr.tail.drain(0..param_num);
            result.tail.append(&mut expr.tail);
            expr.tail = result.tail;
        } else {
            break;
        }
    }
}

// takes an expression M valid in G1, (a + m + e variables)
// and a set of arguments X1..Xm valid in G2 (n variables) where a <= n
// then generates an expression M[x(a+i) <- Xi, x(a+m+i) <- x(n+i)]
fn subst(
    base: &Expr, base_ctx_size: usize, // base = a... arg = n... confusing!
    args: &[Expr], arg_ctx_size: usize,
) -> Expr {
    let subst_on = |xs: &Vec<Expr>| xs
        .iter()
        .map(|x| subst(x, base_ctx_size, args, arg_ctx_size))
        .collect::<Vec<Expr>>();
    let mut arrow_params = subst_on(&base.arrow_params);
    let mut tail = subst_on(&base.tail);
    let head;
    match base.head {
        Ident::Local(i) => {
            if i < base_ctx_size {
                head =  Ident::Local(i);
            } else if i - base_ctx_size < args.len() {
                // @Correctness @Completeness deepen this first
                let mut result = args[i - base_ctx_size].clone();
                arrow_params.append(&mut result.arrow_params);
                head = result.head;
                // @Performance combine these like in eval
                // does Vec::prepend exist?
                // actually we should probably just reverse argument lists
                result.tail.append(&mut tail);
                tail = result.tail;
            } else {
                let e = i - (base_ctx_size + args.len());
                head = Ident::Local(arg_ctx_size + e);
            }
        },
        _ => head = base.head,
    }
    Expr { arrow_params, head, tail }
}
