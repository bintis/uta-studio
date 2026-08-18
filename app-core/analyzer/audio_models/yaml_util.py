"""Restricted YAML loader for catalog and model configs.

Only mappings, sequences, scalars, and a ``!!python/tuple`` constructor
that yields ordinary tuples are accepted. Arbitrary Python object tags
are rejected.
"""

from __future__ import annotations

from typing import Any


class RestrictedYamlError(ValueError):
    pass


def load_restricted_yaml(text: str) -> Any:
    try:
        import yaml
    except ImportError:
        return load_simple_yaml(text)

    class _CatalogLoader(yaml.SafeLoader):
        pass

    def _construct_tuple(loader: yaml.SafeLoader, node: yaml.Node) -> tuple[Any, ...]:
        return tuple(loader.construct_sequence(node))

    _CatalogLoader.add_constructor("tag:yaml.org,2002:python/tuple", _construct_tuple)
    _CatalogLoader.add_constructor("tag:yaml.org,2002:python/tuple", _construct_tuple)

    try:
        return yaml.load(text, Loader=_CatalogLoader)
    except yaml.YAMLError as exc:
        raise RestrictedYamlError(str(exc)) from exc


def load_simple_yaml(text: str) -> Any:
    """Parse a conservative YAML subset used by the shipped catalog."""
    lines: list[tuple[int, str]] = []
    for raw in text.splitlines():
        expanded = raw.replace("\t", "  ")
        if not expanded.strip() or expanded.lstrip().startswith("#"):
            continue
        indent = len(expanded) - len(expanded.lstrip(" "))
        lines.append((indent, expanded.strip()))
    value, _ = _parse_lines(lines, 0, 0)
    return value


def _parse_lines(
    lines: list[tuple[int, str]], index: int, indent: int
) -> tuple[Any, int]:
    if index >= len(lines):
        return None, index
    current_indent, first = lines[index]
    if current_indent != indent:
        raise RestrictedYamlError(f"unexpected indent near {first!r}")
    if first.startswith("- "):
        return _parse_sequence(lines, index, indent)
    return _parse_mapping(lines, index, indent)


def _parse_mapping(
    lines: list[tuple[int, str]], index: int, indent: int
) -> tuple[dict[str, Any], int]:
    mapping: dict[str, Any] = {}
    while index < len(lines):
        current_indent, text = lines[index]
        if current_indent < indent:
            break
        if current_indent > indent:
            raise RestrictedYamlError(f"unexpected nested mapping entry {text!r}")
        if text.startswith("- "):
            raise RestrictedYamlError("cannot mix sequence and mapping at the same indent")
        key, remainder = _split_key(text)
        if remainder == "":
            if index + 1 < len(lines) and lines[index + 1][0] > indent:
                child, index = _parse_lines(lines, index + 1, lines[index + 1][0])
                mapping[key] = child
                continue
            mapping[key] = None
            index += 1
            continue
        mapping[key] = parse_scalar(remainder)
        index += 1
    return mapping, index


def _parse_sequence(
    lines: list[tuple[int, str]], index: int, indent: int
) -> tuple[list[Any], int]:
    items: list[Any] = []
    while index < len(lines):
        current_indent, text = lines[index]
        if current_indent < indent:
            break
        if current_indent > indent or not text.startswith("- "):
            raise RestrictedYamlError(f"expected sequence item, got {text!r}")
        body = text[2:]
        index += 1
        if body == "":
            if index < len(lines) and lines[index][0] > indent:
                child, index = _parse_lines(lines, index, lines[index][0])
                items.append(child)
            else:
                items.append(None)
            continue
        if ":" in body and not _is_quoted(body):
            key, remainder = _split_key(body)
            item: dict[str, Any] = {
                key: parse_scalar(remainder) if remainder != "" else None
            }
            while index < len(lines) and lines[index][0] > indent and not lines[index][1].startswith("- "):
                nested_indent = lines[index][0]
                nested_text = lines[index][1]
                nested_key, nested_remainder = _split_key(nested_text)
                if nested_remainder == "":
                    if index + 1 < len(lines) and lines[index + 1][0] > nested_indent:
                        child, index = _parse_lines(lines, index + 1, lines[index + 1][0])
                        item[nested_key] = child
                        continue
                    item[nested_key] = None
                    index += 1
                    continue
                item[nested_key] = parse_scalar(nested_remainder)
                index += 1
            items.append(item)
            continue
        items.append(parse_scalar(body))
    return items, index


def _split_key(text: str) -> tuple[str, str]:
    if ":" not in text:
        raise RestrictedYamlError(f"expected mapping entry, got {text!r}")
    key, value = text.split(":", 1)
    return key.strip().strip("'\""), value.strip()


def _is_quoted(text: str) -> bool:
    return len(text) >= 2 and text[0] == text[-1] and text[0] in {"'", '"'}


def parse_scalar(text: str) -> Any:
    if text in {"", "~", "null", "Null", "NULL"}:
        return None
    if text in {"true", "True", "TRUE"}:
        return True
    if text in {"false", "False", "FALSE"}:
        return False
    if _is_quoted(text):
        return text[1:-1]
    if text.startswith("[") and text.endswith("]"):
        inner = text[1:-1].strip()
        if not inner:
            return []
        return [parse_scalar(part.strip()) for part in inner.split(",")]
    try:
        if text.startswith(("+", "-")) or text.isdigit() or (
            text.startswith("0") and len(text) == 1
        ):
            return int(text)
        if text.isdecimal():
            return int(text)
    except ValueError:
        pass
    try:
        if any(char in text for char in ".eE"):
            return float(text)
    except ValueError:
        pass
    return text
