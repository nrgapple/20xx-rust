#!/usr/bin/env python3
"""Generate Rust `dat_struct!` definitions from decomp C struct declarations.

Usage:
  gen_dat_structs.py HEADER|DIR [...] -- CName[=RustName] | CName.field:ctype [...] > out.rs

`CName.field:ctype` overrides a field's C type where the decomp's header is
wrong (same size only); record the evidence next to the generated output.

A DIR contributes every `*.h` beneath it; earlier sources win when a name is
defined twice. Structs a requested struct contains are generated too.

Only structs made of scalars, Vec2/Vec3, GXColor, fixed arrays, pointers, and
other structs named on the command line are supported. The generator computes the
layout (big-endian, ILP32, natural alignment) and refuses to emit a struct
whose `/* +OFFSET */` annotations disagree with it; unannotated fields take
the computed offset. Nested structs whose annotations are relative to
the parent (e.g. FallCommon at +520) are rebased to their first field.
"""

import glob
import os
import re
import sys

SCALARS = {
    "float": ("f32", 4), "f32": ("f32", 4),
    "int": ("i32", 4), "s32": ("i32", 4), "enum_t": ("i32", 4), "BOOL": ("i32", 4),
    "bool": ("i32", 4), "FighterKind": ("i32", 4), "HitCapsuleState": ("i32", 4),
    "u32": ("u32", 4), "uint": ("u32", 4), "uintptr_t": ("u32", 4),
    "u16": ("u16", 2), "s16": ("i16", 2),
    "u8": ("u8", 1), "s8": ("i8", 1),
    "Vec2": ("Vec2", 4), "Vec3": ("Vec3", 4),
    "GXColor": ("[u8; 4]", 1),
}
SIZES = {"f32": 4, "i32": 4, "u32": 4, "u16": 2, "i16": 2, "u8": 1, "i8": 1,
         "Vec2": 8, "Vec3": 12, "[u8; 4]": 4, "DatPtr": 4}
KEYWORDS = {"type", "move", "ref", "match", "loop", "box", "fn", "impl", "use", "mod"}

DECL = re.compile(
    r"(?:/\*\s*(?:fp)?\+(?:0[xX])?([0-9A-Fa-f]+)[^*]*\*/\s*)?"  # optional offset annotation
    r"((?:const\s+)?(?:struct\s+|union\s+)?\w+)\s*(\**)\s*"  # type and pointer stars
    r"(\w+|\(\s*\*\s*\w+\s*\))\s*((?:\[[^\]]+\])*)\s*"  # name (or `(*name)`), arrays
    r"(?:DAT_\w+(?:\([^;]*?\))?\s*)*;"  # dat_macros.h tags, e.g. DAT_EXTENT
)


def enum_value(sources, ident):
    """Value of an enumerator, counting from the previous explicit value."""
    for text in sources:
        for body in re.findall(r"enum\s*\w*\s*\{([^}]*)\}", text):
            value = -1
            for item in body.split(","):
                item = re.sub(r"//[^\n]*|/\*.*?\*/", "", item, flags=re.S).strip()
                if not item:
                    continue
                key, _, explicit = item.partition("=")
                try:
                    value = int(explicit.strip(), 0) if explicit.strip() else value + 1
                except (ValueError, TypeError):
                    value = None  # symbolic value: unknown from here on
                if key.strip() == ident and value is not None:
                    return value
    return None


def array_len(expr, name, field, sources=()):
    """Evaluate an array bound made of integer literals, enumerators, and + - * /."""
    for ident in set(re.findall(r"\b[A-Za-z_]\w*\b", expr)) - {"x", "X"}:
        value = enum_value(sources, ident)
        if value is None:
            sys.exit(f"{name}.{field}: unknown array bound {ident}")
        expr = re.sub(r"\b" + ident + r"\b", str(value), expr)
    if not re.fullmatch(r"[\s0-9a-fA-FxX+\-*/()]+", expr):
        sys.exit(f"{name}.{field}: unsupported array bound [{expr}]")
    expr = re.sub(r"\b0[xX]([0-9a-fA-F]+)\b", lambda m: str(int(m.group(1), 16)), expr)
    return eval(expr.replace("/", "//"), {}, {})  # C integer division


def split_declarators(body):
    """Turn `T a, b;` into `T a; T b;` (plain names only)."""
    pattern = re.compile(r"((?:struct\s+|union\s+)?\w+)\s+(\w+(?:\s*,\s*\w+)+)\s*;")
    return pattern.sub(lambda m: " ".join(f"{m.group(1)} {n.strip()};" for n in m.group(2).split(",")), body)


def collapse_inline_structs(body):
    """Turn `struct T { ... } field;` inside a body into `struct T field;`."""
    while True:
        match = re.search(r"((?:struct|union)\s+\w+)\s*\{", body)
        if not match:
            return split_declarators(body)
        depth = 0
        for index in range(match.end() - 1, len(body)):
            depth += {"{": 1, "}": -1}.get(body[index], 0)
            if depth == 0:
                break
        body = body[:match.start()] + match.group(1) + " " + body[index + 1:]


def strip_comments(text):
    text = re.sub(r"/\*\*.*?\*/", "", text, flags=re.S)  # doc blocks
    text = re.sub(r"//[^\n]*", "", text)
    # Drop other block comments, keeping `/* +OFFSET */` annotations.
    text = re.sub(r"/\*(?!\s*(?:fp)?\+[0-9A-Fa-f]).*?\*/", "", text, flags=re.S)
    return text


def find_struct(sources, name):
    pattern = re.compile(r"(?:typedef\s+)?struct\s+" + re.escape(name) + r"\s*\{")
    for text in sources:
        match = pattern.search(text)
        if not match:
            continue
        depth, start = 0, match.end() - 1
        for index in range(start, len(text)):
            depth += {"{": 1, "}": -1}.get(text[index], 0)
            if depth == 0:
                return text[start + 1:index]
    # `typedef struct { ... } Name;`
    for text in sources:
        for match in re.finditer(r"typedef\s+struct\s*\w*\s*\{", text):
            depth, start = 0, match.end() - 1
            for index in range(start, len(text)):
                depth += {"{": 1, "}": -1}.get(text[index], 0)
                if depth == 0:
                    tail = re.match(r"\s*(\w+)\s*;", text[index + 1:])
                    if tail and tail.group(1) == name:
                        return text[start + 1:index]
                    break
    sys.exit(f"struct {name} not found")


def rust_name(cname):
    cname = cname.lstrip("_")
    return cname[0].upper() + cname[1:].replace("_", "")


def has_struct(sources, name):
    pattern = re.compile(r"struct\s+" + re.escape(name) + r"\s*\{|\}\s*" + re.escape(name) + r"\s*;")
    return any(pattern.search(text) for text in sources)


def is_enum(sources, name):
    pattern = re.compile(r"enum\s+" + re.escape(name) + r"\s*\{|typedef\s+enum\s+\w*\s*\{[^}]*\}\s*" + re.escape(name) + r"\s*;|typedef\s+enum\s+\w+\s+" + re.escape(name) + r"\s*;", re.S)
    return any(pattern.search(text) for text in sources)


def snake(name):
    name = name.lower()
    return name + "_" if name in KEYWORDS else name


OVERRIDES = {}


def generate(sources, name, rust_names, emitted, out):
    body = collapse_inline_structs(strip_comments(find_struct(sources, name)))
    fields, base = [], None
    # A declaration DECL cannot parse would silently drop a field.
    unparsed = DECL.sub("", body)
    if ";" in unparsed:
        sys.exit(f"{name}: unparsed declaration near {unparsed.split(';')[0].strip()!r}")
    for match in DECL.finditer(body):
        offset, ctype, stars, field, arrays = match.groups()
        if field.startswith("("):
            # `T (*name)[N]` is one pointer (to an array of N), not an array.
            field, stars, arrays = field.strip("()* \t\n"), stars + "*", ""
        if offset is not None:
            offset = int(offset, 16)
            base = offset if base is None else base
        ctype = re.sub(r"^(const|struct|union)\s+", "", ctype).strip()
        if (name, field.lower()) in OVERRIDES:
            old = SCALARS.get(ctype, (None, None))
            ctype = OVERRIDES.pop((name, field.lower()))
            if stars or SIZES.get(old[0]) != SIZES.get(SCALARS[ctype][0]):
                sys.exit(f"{name}.{field}: override must keep a scalar's size")
        counts = [array_len(n, name, field, sources) for n in re.findall(r"\[([^\]]+)\]", arrays)]
        if stars:
            rtype, align = "DatPtr", 4
        elif ctype in SCALARS:
            rtype, align = SCALARS[ctype]
        elif is_enum(sources, ctype):
            rtype, align = "i32", 4  # MWCC enums are 4 bytes (-enum int)
        elif ctype in rust_names or has_struct(sources, ctype):
            # Nested structs are generated on demand under a derived name.
            rust_names.setdefault(ctype, rust_name(ctype))
            if ctype not in emitted:
                generate(sources, ctype, rust_names, emitted, out)
            rtype, align = rust_names[ctype], emitted[ctype][1]
        else:
            sys.exit(f"{name}.{field}: unsupported type {ctype}")
        size = SIZES.get(rtype) or emitted[ctype][0]
        for count in reversed(counts):
            rtype, size = f"[{rtype}; {count}]", size * count
        fields.append([None if offset is None else offset - base, snake(field), rtype, size, align])
    if not fields:
        sys.exit(f"struct {name} has no fields")
    # Lay out fields naturally, then compare with the decomp's annotations. A
    # lone wrong annotation (a typo) is accepted with a warning when the next
    # annotated field agrees with the computed layout; anything else fails.
    cursor, max_align = 0, 1
    for entry in fields:
        align, size = entry[4], entry[3]
        cursor = (cursor + align - 1) // align * align
        entry.append(cursor)
        cursor += size
        max_align = max(max_align, align)
    annotated = [entry for entry in fields if entry[0] is not None]
    for index, entry in enumerate(annotated):
        offset, field, computed = entry[0], entry[1], entry[5]
        if offset == computed:
            continue
        following = annotated[index + 1] if index + 1 < len(annotated) else None
        if following is None or following[0] != following[5]:
            sys.exit(f"{name}.{field}: annotation +{offset:#x} != computed +{computed:#x}")
        print(f"warning: {name}.{field} annotated +{offset:#x}; layout and the next "
              f"annotation ({following[1]} at +{following[0]:#x}) give +{computed:#x}",
              file=sys.stderr)
    for entry in fields:
        entry[0] = entry.pop()
    total = (cursor + max_align - 1) // max_align * max_align
    emitted[name] = (total, max_align)
    out.append(f"dat_struct! {{\n    /// `{name}` (decomp `src/melee`), {total:#x} bytes.\n")
    out.append(f"    pub struct {rust_names[name]} {{\n")
    for offset, field, rtype, _, _ in fields:
        out.append(f"        {offset:#05x} {field}: {rtype},\n")
    out.append(f"    }}\n    size {total:#x}\n}}\n\n")


def main():
    if "--" not in sys.argv:
        sys.exit(__doc__)
    split = sys.argv.index("--")
    sources = []
    for path in sys.argv[1:split]:
        paths = sorted(glob.glob(os.path.join(path, "**", "*.h"), recursive=True)) if os.path.isdir(path) else [path]
        sources += [open(p, errors="replace").read() for p in paths]
    requested = []
    rust_names = {}
    for spec in sys.argv[split + 1:]:
        if "." in spec:
            target, _, ctype = spec.partition(":")
            struct, _, field = target.partition(".")
            OVERRIDES[(struct, field.lower())] = ctype
            continue
        cname, _, rname = spec.partition("=")
        rust_names[cname] = rname or rust_name(cname)
        requested.append(cname)
    out, emitted = [], {}
    for cname in requested:
        if cname not in emitted:
            generate(sources, cname, rust_names, emitted, out)
    if OVERRIDES:
        sys.exit(f"unused overrides: {OVERRIDES}")
    sys.stdout.write("".join(out))


if __name__ == "__main__":
    main()
