"""A KDL 2 lexer for Pygments, which ships no KDL lexer.

hooks.py registers it, so a ```kdl fence is highlighted.
"""

from pygments.lexer import RegexLexer, bygroups
from pygments.token import (
    Comment,
    Keyword,
    Name,
    Number,
    Operator,
    Punctuation,
    String,
    Whitespace,
)

__all__ = ["KdlLexer"]

IDENT = r"[^\s\\/(){}\[\]<>;=,\"#]+"
NUMBER = r"[-+]?(?:0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+|[0-9][0-9_]*(?:\.[0-9_]+)?(?:[eE][-+]?[0-9_]+)?)"
KEYWORD = r"#(?:true|false|null|inf|-inf|nan)\b"

COMMON = [
    (r"//.*?$", Comment.Single),
    (r"/\*", Comment.Multiline, "comment"),
    (r"/-", Comment.Special),
    (r"\([^)]*\)", Name.Decorator),
]


class KdlLexer(RegexLexer):
    name = "KDL"
    aliases = ["kdl"]
    filenames = ["*.kdl"]

    tokens = {
        "root": COMMON
        + [
            (r"\s+", Whitespace),
            (r"[{};]", Punctuation),
            (r'"(?:\\.|[^"\\])*"', Name.Tag, "entries"),
            (IDENT, Name.Tag, "entries"),
        ],
        "entries": COMMON
        + [
            (r"[ \t]+", Whitespace),
            (r"\\\s*\n", Whitespace),
            (r"\n", Whitespace, "#pop"),
            (r";", Punctuation, "#pop"),
            (r"\{", Punctuation, "#pop"),
            (r"\}", Punctuation, "#pop"),
            (r"(" + IDENT + r")(=)", bygroups(Name.Attribute, Operator)),
            (r'(#+)".*?"\1', String),
            (r'"(?:\\.|[^"\\])*"', String.Double),
            (KEYWORD, Keyword.Constant),
            (NUMBER, Number),
            (IDENT, Name),
        ],
        "comment": [
            (r"/\*", Comment.Multiline, "#push"),
            (r"\*/", Comment.Multiline, "#pop"),
            (r"[^/*]+", Comment.Multiline),
            (r"[/*]", Comment.Multiline),
        ],
    }
