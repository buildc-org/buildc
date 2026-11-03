"""Serve the repository's root documents as site pages beside the pages under docs/.

MkDocs loads this file through `hooks:` in mkdocs.yml.
"""

import importlib.util
import re
import sys
from pathlib import Path

from mkdocs.structure.files import File
from pygments.lexers import LEXERS

_spec = importlib.util.spec_from_file_location("kdl_lexer", Path(__file__).with_name("kdl_lexer.py"))
sys.modules["kdl_lexer"] = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sys.modules["kdl_lexer"])
LEXERS["KdlLexer"] = ("kdl_lexer", "KDL", ("kdl",), ("*.kdl",), ())

PAGES = {
    "README.md": "index.md",
    "CHANGELOG.md": "changelog.md",
    "CONTRIBUTING.md": "contributing.md",
}
BLOB = "https://github.com/buildc-org/buildc/blob/main/"
ALERT = re.compile(r"^> \[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\n((?:>.*\n?)*)", re.MULTILINE)
LINK = re.compile(r"\]\((?!https?://|#|mailto:)([^)\s]+)\)")


def rebase(target: str) -> str:
    if target.startswith("docs/"):
        return target.removeprefix("docs/")
    if target in PAGES:
        return PAGES[target]
    return f"{BLOB}{target}"


def on_files(files, config):
    root = Path(config.config_file_path).parent
    for source, page in PAGES.items():
        path = root / source
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        text = LINK.sub(lambda m: f"]({rebase(m.group(1))})", text)
        files.append(File.generated(config, page, content=text))
    return files


STYLE = {"NOTE": "note", "TIP": "info", "IMPORTANT": "warning", "WARNING": "warning", "CAUTION": "danger"}


def admonition(match: re.Match) -> str:
    body = "".join(f"    {line[2:] if line.startswith('> ') else line[1:]}\n" for line in match.group(2).splitlines())
    kind = match.group(1)
    return f'!!! {STYLE[kind]} "{kind.capitalize()}"\n{body}\n'


def on_page_markdown(markdown, page, config, files):
    return ALERT.sub(admonition, markdown)
