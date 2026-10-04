; Queries for tree-sitter-perl 1.1.2, whose crate omits a highlight query.
(comments) @comment
(pod_statement) @comment.doc
[
  (string_double_quoted)
  (string_single_quoted)
  (string_q_quoted)
  (string_qq_quoted)
  (heredoc_body_statement)
  (word_list_qw)
] @string
(escape_sequence) @string.escape
[
  (regex_pattern_content)
  (regex_pattern_qr)
] @string.regex
[
  (integer)
  (floating_point)
  (scientific_notation)
  (hexadecimal)
  (octal)
] @number
[
  (scalar_variable)
  (special_scalar_variable)
  (array_variable)
  (hash_variable)
  (package_variable)
] @variable
[
  (package_name)
  (module_name)
] @type
(function_definition (identifier) @function)
[
  "my"
  "our"
  "local"
  "sub"
  "package"
  "use"
  "no"
  "require"
  "if"
  "elsif"
  "else"
  "unless"
  "while"
  "until"
  "for"
  "foreach"
  "return"
  "continue"
] @keyword
(loop_control_keyword) @keyword
[(true) (false)] @boolean
