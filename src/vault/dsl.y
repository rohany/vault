%start Expr
%avoid_insert "INT"
%avoid_insert "FLOAT"
%left '+' '-' '*' '/' '%' 'OR' 'AND' '::' '==' '=' '>' '>=' '<' '<='
%%

// TODO (rohany): Should this be Result<T, ()>?
Const -> Result<Const, ()>:
      'TRUE' { Ok(Const::Bool{ b: true }) }
    | 'FALSE' { Ok(Const::Bool{ b: false }) }
    | 'NULL' { Ok(Const::Null) }
    | 'STRING'
      {
        // TODO (rohany): We could keep this as a span, or hold a
        //  reference to the string that was passed in from parsing.
        let v = $1.map_err(|_| ())?;
        let x = $lexer.span_str(v.span());
        Ok(Const::String{ s: x.to_string() })
      }
    | 'INT'
      {
        let v = $1.map_err(|_| ())?;
        let i = parse_constant($lexer.span_str(v.span()))?;
        Ok(Const::Int{ i })
      }
    | 'FLOAT'
      {
        let v = $1.map_err(|_| ())?;
        let f = parse_constant($lexer.span_str(v.span()))?;
        Ok(Const::Float{ f })
      }
    ;

MetaAccess -> Result<MetaAccess, ()>:
      'META' '[' 'STRING' ']'
      {
        let v = $3.map_err(|_| ())?;
        let field = $lexer.span_str(v.span());
        Ok(MetaAccess::Access{ base: Box::new(MetaAccess::Meta), field: field.to_string() })
      }
    | MetaAccess '[' 'STRING' ']'
      {
        let v = $3.map_err(|_| ())?;
        let field = $lexer.span_str(v.span());
        Ok(MetaAccess::Access{ base: Box::new($1?), field: field.to_string() })
      }
    ;

Expr -> Result<Box<Expr>, ()>:
      Const { Ok(Box::new(Expr::Const{ c: $1? })) }
    | '(' Expr ')' { Ok(Box::new(Expr::ParenExpr{ expr: $2? })) }
    | MetaAccess { Ok(Box::new(Expr::MetaAccess{ m: $1? })) }
    // TODO (rohany): Since we're already leaning to Pythonic syntax, maybe we
    //  should get rid of this kind of cast?
    | Expr '::' 'NAME'
      {
        let v = $3.map_err(|_| ())?;
        let typename = $lexer.span_str(v.span());
        Ok(Box::new(Expr::Cast{
          expr: $1?,
          typ: typename.to_string(),
        }))
      }
    | 'NAME' '(' ExprList ')'
      {
        let v = $1.map_err(|_| ())?;
        let funcname = $lexer.span_str(v.span());
        Ok(Box::new(Expr::Ap{
          f: funcname.to_string(),
          args: $3?,
        }))
      }
    // Note that we can't pull the operation into a separate rule because
    // Yacc relies on the inlined token to perform precedence analysis.
    | Expr '+' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Plus, r: $3? })) }
    | Expr '*' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Times, r: $3? })) }
    | Expr '-' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Sub, r: $3? })) }
    | Expr '/' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Div, r: $3? })) }
    | Expr '%' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Mod, r: $3? })) }
    | Expr 'AND' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::And, r: $3? })) }
    | Expr 'OR' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Or, r: $3? })) }
    // We will support both `==` and `=` to mean Eq.
    | Expr '==' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Eq, r: $3? })) }
    | Expr '=' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Eq, r: $3? })) }
    | Expr '>' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Ge, r: $3? })) }
    | Expr '>=' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Geq, r: $3? })) }
    | Expr '<' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Le, r: $3? })) }
    | Expr '<=' Expr { Ok(Box::new(Expr::BinOp{ l: $1?, op: Op::Leq, r: $3? })) }
    ;

ExprList -> Result<Vec<Box<Expr>>, ()>:
      Expr { Ok(vec![$1?]) }
    | ExprList ',' Expr
      {
        let mut l = $1?;
        l.push($3?);
        Ok(l)
      }
    ;
%%

use crate::vault::dsl::ast::*;

fn parse_constant<T: std::str::FromStr>(s: &str) -> Result<T, ()> {
    match s.parse::<T>() {
        Ok(val) => Ok(val),
        Err(_) => {
            // TODO (rohany): How can I get the type T here?
            eprintln!("{} cannot be represented as desired type", s);
            Err(())
        }
    }
}
