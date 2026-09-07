#!/usr/bin/env python3
"""Write Mewo logo concept SVGs and a contact sheet."""

from pathlib import Path

ROOT = Path(__file__).resolve().parent

SVG = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="512" height="512">
  <rect width="100" height="100" fill="#000"/>
  {mark}
</svg>
"""

concepts = {
    "1-closed-book-x": (
        "Closed book, X cut from the spine",
        """<path fill="#fff" fill-rule="evenodd" d="
          M26 30 L70 24 L74 78 L28 82 Z
          M54 29 L66 29 L69 46 L86 29 L95 29 L74 52 L95 77 L84 77 L69 58 L54 77 L44 77 L65 52 L50 29 Z
        "/>""",
    ),
    "2-open-book-cutouts": (
        "Open book, even-odd voids",
        """<path fill="#fff" fill-rule="evenodd" d="
          M18 34 L48 28 L48 78 L20 74 Z
          M26 42 L41 38 L41 66 L27 64 Z
          M52 28 L82 34 L80 74 L52 78 Z
          M59 38 L74 42 L73 64 L59 66 Z
        "/>""",
    ),
    "3-x-as-book": (
        "X that is also an open book",
        """<path fill="#fff" fill-rule="evenodd" d="
          M22 22 L42 22 L50 42 L58 22 L78 22 L54 50 L78 78 L58 78 L50 58 L42 78 L22 78 L46 50 Z
          M32 30 L42 30 L50 46 L58 30 L68 30 L50 52 L68 70 L58 70 L50 54 L42 70 L32 70 L50 48 Z
        "/>""",
    ),
    "4-closed-book-outline": (
        "Front-on closed book, outline",
        """<g fill="none" stroke="#fff" stroke-width="3.2" stroke-linejoin="miter" stroke-linecap="square">
          <path d="M32 24 L68 24 L68 70 L54 70 L62 82 L32 82 Z"/>
          <path d="M40 24 L40 82"/>
          <path d="M32 24 L36 20 L44 20"/>
          <path d="M54 70 L62 70 L62 82"/>
        </g>""",
    ),
    "5-open-book-filled": (
        "Open book, filled pages",
        """<path fill="#fff" fill-rule="evenodd" d="
          M20 36 L48 28 L48 78 L22 72 Z
          M20 36 L30 36 L20 48 Z
          M52 28 L80 36 L78 72 L52 78 Z
          M80 36 L70 36 L80 48 Z
        "/>
        <path fill="none" stroke="#fff" stroke-width="2.4" d="
          M18 32 L18 74 L22 72
          M82 32 L82 74 L78 72
        "/>""",
    ),
}


def main() -> None:
    names = []
    for key, (title, mark) in concepts.items():
        path = ROOT / f"{key}.svg"
        path.write_text(SVG.format(mark=mark.strip()), encoding="utf-8")
        names.append((key, title, path))
        print(f"wrote {path.name}  ({title})")

    captions = ROOT / "captions.txt"
    captions.write_text(
        "\n".join(f"{key}.png\t{title}" for key, title, _ in names) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
