use super::*;

fn parse_ruby(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_ruby::LANGUAGE.into();
    parser.set_language(&lang).unwrap();
    parser.parse(source, None).unwrap()
}

fn symbols_of(source: &str) -> Vec<Symbol> {
    let tree = parse_ruby(source);
    extract_ruby("/test.rb", source, tree.root_node()).1
}

/// The visibility of the symbol called `name`; panics when it is missing.
fn visibility_of(symbols: &[Symbol], name: &str) -> Visibility {
    symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no symbol named {name} in {symbols:?}"))
        .visibility
}

#[test]
fn test_ruby_class_extraction() {
    let source = "\n# A greeter class\nclass Greeter\n  def greet(name)\n    \"Hello, #{name}\"\n  end\nend\n";
    let symbols = symbols_of(source);

    assert!(symbols
        .iter()
        .any(|s| s.name == "Greeter" && s.kind == SymbolKind::Class));
    assert!(symbols
        .iter()
        .any(|s| s.name == "greet" && s.kind == SymbolKind::Function));
}

#[test]
fn test_ruby_module_extraction() {
    let source = r#"
module Utils
  def self.helper
    true
  end
end
"#;
    let symbols = symbols_of(source);

    assert!(symbols
        .iter()
        .any(|s| s.name == "Utils" && s.kind == SymbolKind::Module));
    assert!(symbols
        .iter()
        .any(|s| s.name == "self.helper" && s.kind == SymbolKind::Function));
}

#[test]
fn test_ruby_constant_extraction() {
    let symbols = symbols_of("MAX_SIZE = 1024\n");

    assert!(symbols
        .iter()
        .any(|s| s.name == "MAX_SIZE" && s.kind == SymbolKind::Const));
}

#[test]
fn test_ruby_require_relative() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib = dir.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("helper.rb"), "module Helper; end").unwrap();
    let from_file = lib.join("main.rb").to_str().unwrap().to_string();

    let source = "require_relative 'helper'\n";
    let tree = parse_ruby(source);
    let (imports, _) = extract_ruby(&from_file, source, tree.root_node());

    assert_eq!(imports.len(), 1);
    assert!(imports[0].target.ends_with("helper.rb"));
}

#[test]
fn test_ruby_doc_comment() {
    let source = r#"
# Calculate the sum of two numbers.
# Returns an integer.
def add(a, b)
  a + b
end
"#;
    let symbols = symbols_of(source);

    let sym = symbols.iter().find(|s| s.name == "add").unwrap();
    assert!(sym.doc_comment.is_some());
    let doc = sym.doc_comment.as_ref().unwrap();
    assert!(doc.contains("Calculate the sum"));
}

#[test]
fn bare_private_applies_to_the_methods_after_it() {
    let symbols = symbols_of(
        "class A\n  def before; end\n  private\n  def hidden; end\n  attr_reader :secret\n  public\n  def after; end\nend\n",
    );

    assert_eq!(visibility_of(&symbols, "before"), Visibility::Public);
    assert_eq!(visibility_of(&symbols, "hidden"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "secret"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "after"), Visibility::Public);
}

#[test]
fn bare_private_ends_with_the_body_it_is_in() {
    let symbols = symbols_of(
        "class A\n  private\n  class Inner\n    def inner_method; end\n  end\n  def hidden; end\nend\nclass B\n  def open; end\nend\n",
    );

    assert_eq!(visibility_of(&symbols, "inner_method"), Visibility::Public);
    assert_eq!(visibility_of(&symbols, "hidden"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "open"), Visibility::Public);
}

#[test]
fn private_with_names_changes_methods_already_defined() {
    let symbols = symbols_of(
        "class A\n  def one; end\n  def two; end\n  def three; end\n  private :one, 'two'\nend\n",
    );

    assert_eq!(visibility_of(&symbols, "one"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "two"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "three"), Visibility::Public);
}

#[test]
fn private_with_names_does_not_reach_into_another_class() {
    let symbols = symbols_of(
        "class A\n  def shared; end\nend\nclass B\n  def other; end\n  private :shared\nend\n",
    );

    assert_eq!(visibility_of(&symbols, "shared"), Visibility::Public);
}

#[test]
fn private_in_front_of_a_definition_applies_to_that_definition_only() {
    let symbols = symbols_of(
        "class A\n  private def inline; end\n  private attr_accessor :token\n  def after; end\nend\n",
    );

    assert_eq!(visibility_of(&symbols, "inline"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "token"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "after"), Visibility::Public);
}

#[test]
fn class_methods_are_private_only_through_private_class_method() {
    let symbols = symbols_of(
        "class A\n  private\n  def self.open; end\n  def self.build; end\n  private_class_method :build\nend\n",
    );

    assert_eq!(visibility_of(&symbols, "self.open"), Visibility::Public);
    assert_eq!(visibility_of(&symbols, "self.build"), Visibility::Private);
}

#[test]
fn a_class_method_defined_as_the_argument_of_private_class_method_is_private() {
    let symbols = symbols_of("class A\n  private_class_method def self.build; end\nend\n");

    assert_eq!(visibility_of(&symbols, "self.build"), Visibility::Private);
}

#[test]
fn protected_methods_are_not_public() {
    let symbols =
        symbols_of("class A\n  protected\n  def guarded; end\n  public\n  def open; end\nend\n");

    assert_eq!(visibility_of(&symbols, "guarded"), Visibility::Private);
    assert_eq!(visibility_of(&symbols, "open"), Visibility::Public);
}
