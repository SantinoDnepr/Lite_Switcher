"""Removes the elements of the page generator from the help pages in Help/.

The pages are made with the `answer-me-with-html` skill from Markdown drafts. The generator adds a credit line
with links, an update notice and the buttons for collecting a reply for an agent (Reply, Remark, Copy source).
None of them belongs in the help of this program. The theme and the light/dark mode switches stay.

usage: python tools/help/clean-help.py [Help folder]
"""

import re
import sys
from pathlib import Path


# the labels of the theme and mode switches (the generator writes them in English for these languages)
SWITCHES = {
    "ru": {"Theme": "Тема", "Mode": "Режим", "Auto": "Авто", "Light": "Светлый", "Dark": "Тёмный",
           "Blueprint": "Чертёж", "Cards": "Карточки", "Paper": "Бумага"},
    "uk": {"Theme": "Тема", "Mode": "Режим", "Auto": "Авто", "Light": "Світлий", "Dark": "Темний",
           "Blueprint": "Креслення", "Cards": "Картки", "Paper": "Папір"},
}


def localize_switches(html: str, lang: str) -> str:
    names = SWITCHES.get(lang)
    if not names:
        return html
    toolbar = re.search(r'<div class="am-toolbar".*?</div>', html, flags=re.S)
    if not toolbar:
        return html
    block = toolbar.group(0)
    block = re.sub(r'(<label class="am-pick">)(\w+)', lambda m: m.group(1) + names.get(m.group(2), m.group(2)), block)
    block = re.sub(r'(<option value="[a-z]+"(?: selected)?>)([^<]+)', lambda m: m.group(1) + names.get(m.group(2), m.group(2)), block)
    return html[: toolbar.start()] + block + html[toolbar.end():]


def clean(html: str) -> str:
    # the toolbar buttons: the update notice, Reply, Remark and Copy source
    for kind in ("update", "reply", "remark"):
        html = re.sub(r'<button class="am-btn am-btn--%s"[^>]*>.*?</button>\n?' % kind, "", html, flags=re.S)
    html = re.sub(r'<button class="am-btn" type="button" data-am="copy" [^>]*>.*?</button>\n?', "", html, flags=re.S)
    # the update popover (from its opening tag to the end of the toolbar block that follows)
    html = re.sub(r'<div class="am-update" id="am-update" popover>.*?</div>\n(?=<main)', "", html, flags=re.S)
    # the credit line with the links
    html = re.sub(r'<p class="am-colophon">.*?</p>', "", html, flags=re.S)
    lang = re.search(r'<html lang="([^"]+)"', html)
    return localize_switches(html, lang.group(1) if lang else "en")


def main() -> None:
    folder = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2] / "Help"
    for page in sorted(folder.glob("Help_*.html")):
        text = page.read_text(encoding="utf-8")
        cleaned = clean(text)
        page.write_text(cleaned, encoding="utf-8", newline="")
        print(f"{page.name}: {len(text)} -> {len(cleaned)} bytes")


if __name__ == "__main__":
    main()
