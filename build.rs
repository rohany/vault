use cfgrammar::yacc::YaccKind;
use lrlex::LexerBuilder;
use lrpar::CTParserBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The main build.rs file will compile the Lex and Yacc files for use.
    let lex_rule_ids_map = CTParserBuilder::new()
        .yacckind(YaccKind::Grmtools)
        .process_file_in_src("vault/dsl.y")?;
    LexerBuilder::new()
        .rule_ids_map(lex_rule_ids_map)
        .process_file_in_src("vault/dsl.l")?;
    Ok(())
}
