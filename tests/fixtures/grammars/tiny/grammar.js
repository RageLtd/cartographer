// Minimal grammar used as a test fixture for Cartographer's dynamic grammar loader
// and query-driven extractor. Deliberately tiny: imports, functions, visibility,
// and doc comments are all it knows.
module.exports = grammar({
  name: 'tiny',

  extras: $ => [/\s/, $.comment],

  rules: {
    source_file: $ => repeat(choice($.import_statement, $.function_definition)),

    comment: $ => token(seq('#', /.*/)),

    import_statement: $ => seq('use', field('path', $.string), ';'),

    function_definition: $ => seq(
      optional(field('visibility', $.pub)),
      'fn',
      field('name', $.identifier),
      '(',
      ')',
      $.block,
    ),

    pub: $ => 'pub',

    block: $ => seq('{', '}'),

    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,

    string: $ => /"[^"]*"/,
  },
});
