use crate::vault::dsl::trans::ToSQLite;
use lrlex::lrlex_mod;
use lrpar::lrpar_mod;

// Include the lexer and parser defined in dsl.l and dsl.y.
lrlex_mod!("vault/dsl.l");
lrpar_mod!("vault/dsl.y");

/// query_string_to_sqlite converts a string in the Vault query DSL
/// to a SQLite SQL syntax predicate.
pub fn query_string_to_sqlite(s: &str) -> Result<String, String> {
    let expr = parse_expr(s)?;
    let mut result = String::new();
    expr.to_sqlite(&mut result);
    Ok(result)
}

/// ast contains the AST for the Vault query DSL.
mod ast {
    use std::fmt;

    /// Const is a leaf in the AST. It represents constants like Integers, Floats, etc.
    #[derive(Debug)]
    pub enum Const {
        Int { i: i64 },
        Float { f: f64 },
        String { s: String },
        Null,
        Bool { b: bool },
    }

    impl fmt::Display for Const {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Const::Int { i } => write!(f, "{}", i),
                Const::Float { f: fl } => write!(f, "{}", fl),
                Const::String { s } => write!(f, "{}", s),
                Const::Null => write!(f, "null"),
                Const::Bool { b } => write!(f, "{}", b),
            }
        }
    }

    /// Op represents different kinds of supported binary operators.
    #[derive(Debug)]
    pub enum Op {
        // Arithmetic operators.
        Plus,
        Times,
        Div,
        Sub,
        Mod,
        // Logical operators.
        Or,
        And,
        // Comparison operators.
        Eq,
        Ge,
        Geq,
        Le,
        Leq,
    }

    impl fmt::Display for Op {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Op::Plus => write!(f, "+"),
                Op::Times => write!(f, "*"),
                Op::Div => write!(f, "/"),
                Op::Sub => write!(f, "-"),
                Op::Mod => write!(f, "%"),
                Op::Or => write!(f, "OR"),
                Op::And => write!(f, "AND"),
                Op::Eq => write!(f, "=="),
                Op::Ge => write!(f, ">"),
                Op::Geq => write!(f, ">="),
                Op::Le => write!(f, "<"),
                Op::Leq => write!(f, "<="),
            }
        }
    }

    /// MetaAccess represents an access to the metadata for an experiment.
    /// Accesses to the metadata happen through an access to the `meta` variable.
    #[derive(Debug)]
    pub enum MetaAccess {
        Meta,
        Access {
            base: Box<MetaAccess>,
            field: String,
        },
    }

    impl fmt::Display for MetaAccess {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                MetaAccess::Meta => write!(f, "meta"),
                MetaAccess::Access { base, field } => write!(f, "{}[{}]", base, field),
            }
        }
    }

    /// Expr represents different kinds of expressions in the DSL.
    #[derive(Debug)]
    pub enum Expr {
        Const { c: Const },
        BinOp { l: Box<Expr>, op: Op, r: Box<Expr> },
        Ap { f: String, args: Vec<Box<Expr>> },
        MetaAccess { m: MetaAccess },
        Cast { expr: Box<Expr>, typ: String },
        ParenExpr { expr: Box<Expr> },
    }

    impl fmt::Display for Expr {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Expr::Const { c } => c.fmt(f),
                Expr::BinOp { l, op, r } => write!(f, "{} {} {}", l, op, r),
                Expr::Ap { f: func, args } => {
                    write!(f, "{}(", func)?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?
                        }
                        write!(f, "{}", arg)?
                    }
                    write!(f, ")")
                }
                Expr::MetaAccess { m } => m.fmt(f),
                Expr::Cast { expr, typ } => write!(f, "{}::{}", expr, typ),
                Expr::ParenExpr { expr } => write!(f, "({})", expr),
            }
        }
    }
}

/// parse_expr parses and input string and returns the ast::Expr that
/// corresponds to the input string.
// TODO (rohany): Make this error not a string?
fn parse_expr(s: &str) -> Result<Box<ast::Expr>, String> {
    let lexerdef = dsl_l::lexerdef();
    let lexer = lexerdef.lexer(s);
    let (res, errs) = dsl_y::parse(&lexer);
    if errs.len() > 0 {
        let mut build = String::new();
        build.push_str("Error: ");
        for e in errs {
            build.push_str(&format!("{}\n", e));
        }
        return Err(build);
    };
    // TODO (rohany): If we get past here, can we guarantee that we don't get None?
    Ok(res.unwrap().unwrap())
}

/// trans contains logic relating to translation of the Vault DSL into
/// SQLite SQL predicate syntax.
mod trans {
    use crate::vault::dsl::ast::*;

    /// ToSQLite is a trait for conversion into a string of valid SQLite
    /// SQL predicate syntax.
    // TODO (rohany): This is a bit crude, the direct translation to a string.
    //  In the future, we should make this translation to a different AST, which
    //  then implements fmt::Display to print in SQLite SQL syntax.
    pub trait ToSQLite {
        /// to_sqlite takes in a mutable string buffer where the translated
        /// SQL should be written to.
        fn to_sqlite(&self, s: &mut String);
    }

    impl ToSQLite for Expr {
        // All translations for Expr are straightforward.
        fn to_sqlite(&self, buf: &mut String) {
            match self {
                Expr::Const { c } => c.to_sqlite(buf),
                Expr::BinOp { l, op, r } => {
                    l.to_sqlite(buf);
                    buf.push_str(" ");
                    op.to_sqlite(buf);
                    buf.push_str(" ");
                    r.to_sqlite(buf);
                }
                Expr::Ap { f, args } => {
                    buf.push_str(format!("{}(", f).as_str());
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            buf.push_str(", ");
                        }
                        arg.to_sqlite(buf);
                    }
                    buf.push_str(")")
                }
                Expr::MetaAccess { m } => m.to_sqlite(buf),
                Expr::Cast { expr, typ } => {
                    // The cast translation turns x::t into CAST(x AS t).
                    buf.push_str("CAST(");
                    expr.to_sqlite(buf);
                    buf.push_str(format!(" AS {})", typ).as_str())
                }
                Expr::ParenExpr { expr } => {
                    buf.push_str("(");
                    expr.to_sqlite(buf);
                    buf.push_str(")")
                }
            }
        }
    }

    impl ToSQLite for Op {
        fn to_sqlite(&self, buf: &mut String) {
            match self {
                Op::Plus => buf.push_str("+"),
                Op::Times => buf.push_str("*"),
                Op::Div => buf.push_str("/"),
                Op::Sub => buf.push_str("-"),
                Op::Mod => buf.push_str("%"),
                Op::Or => buf.push_str("OR"),
                Op::And => buf.push_str("AND"),
                Op::Eq => buf.push_str("="),
                Op::Ge => buf.push_str(">"),
                Op::Geq => buf.push_str(">="),
                Op::Le => buf.push_str("<"),
                Op::Leq => buf.push_str("<="),
            }
        }
    }

    impl ToSQLite for Const {
        fn to_sqlite(&self, buf: &mut String) {
            match self {
                Const::Null => buf.push_str("NULL"),
                Const::Int { i } => buf.push_str(format!("{}", i).as_str()),
                Const::Float { f } => buf.push_str(format!("{}", f).as_str()),
                // TODO (rohany): This case isn't right. We need to escape the inside of the string
                //  so that we aren't vulnerable to any SQL injection attacks. We could snag the
                //  code that does this from CockroachDB's lex package.
                Const::String { s } => buf.push_str(format!("{}", s).as_str()),
                Const::Bool { b } => buf.push_str(format!("{}", b).as_str()),
            }
        }
    }

    impl ToSQLite for MetaAccess {
        fn to_sqlite(&self, buf: &mut String) {
            match self {
                MetaAccess::Meta => buf.push_str("meta"),
                MetaAccess::Access { base, field } => {
                    // The translation of MetaAccess is the only interesting part of
                    // this translation. We translate from a python style dictionary
                    // access (`d[k]`) into the function access pattern that SQLite
                    // offers. This translates to `json_extract(d, '$.k')`.
                    buf.push_str("json_extract(");
                    base.to_sqlite(buf);
                    // The raw string token received from the parser contains the
                    // single quotes that surround it. We don't want those, so strip
                    // them away before formatting in the key.
                    let key = &field[1..(field.len() - 1)];
                    buf.push_str(format!(", '$.{}')", key).as_str())
                }
            }
        }
    }
}

#[cfg(test)]
mod parse_test {
    use super::*;

    #[test]
    fn parse_and_round_trip() {
        let tests = vec![
            "5",
            "-5",
            "'a'",
            "5.01",
            "-5.01",
            "'a4-*0/+-*'",
            "null",
            "false",
            "true",
            "5 + 5",
            "5 - 5",
            "5 * 5",
            "5 / 5",
            "5 % 5",
            "true OR false",
            "true AND false",
            "5 == 5",
            "5 > 5",
            "5 >= 5",
            "5 < 5",
            "5 <= 5",
            "(5 + 5) * 10 / (1 - 15) % 3",
            "((((5 + 5))))",
            "(5 + 5)::float",
            "func1(1)",
            "func1(5, 7, 8) + func2(1)",
            "meta['key1']['key2']",
        ];

        // Ensure that each expression in tests round trips through the parser.
        for test in tests.iter() {
            match parse_expr(test) {
                Ok(expr) => {
                    // Ensure that we round trip.
                    assert_eq!(&format!("{}", expr), test)
                }
                Err(e) => panic!("Parsing {} failed with error {}", test, e),
            }
        }
    }

    #[test]
    fn parse_and_no_round_trip() {
        let tests = vec![("5\n", "5"), ("5 = 5", "5 == 5")];
        // Ensure that each expression in tests parses and equals the target value.
        for (test, expected) in tests.iter() {
            match parse_expr(test) {
                Ok(expr) => {
                    // Ensure that we round trip.
                    assert_eq!(&format!("{}", expr), expected)
                }
                Err(e) => panic!("Parsing {} failed with error {}", test, e),
            }
        }
    }
}

#[cfg(test)]
mod trans_test {
    use super::*;
    use crate::vault::dsl::trans::ToSQLite;
    use datadriven::walk;

    #[test]
    fn datadriven() {
        walk("tests/testdata/vault/dsl/translate", |f| {
            f.run(|test_case| -> String {
                match test_case.directive.as_str() {
                    "trans" => {
                        let expr = match parse_expr(&test_case.input) {
                            Ok(e) => e,
                            Err(err) => return format!("Error: {}\n", err),
                        };
                        let mut result = String::new();
                        expr.to_sqlite(&mut result);
                        result.push_str("\n");
                        result
                    }
                    _ => panic!("Unhandled directive: {}", &test_case.directive),
                }
            })
        })
    }
}
