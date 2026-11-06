#!/usr/bin/env python3
"""Generate a tiny ONNX probe model WITHOUT the `onnx` package.

The model is ReduceMean over the channel axis: [1,3,512,512] -> [1,1,512,512].
It performs real arithmetic (not a Constant), so loading and running it through
`tract` exercises the same path as a real segmentation model: model parse,
input fact overriding, optimization, inference, output shape inspection and
the tiled/feathered assembly in `segmentation::segment`.

The ONNX file is hand-encoded protobuf so the fixture is reproducible with only
CPython's standard library. Run: `python make_probe_model.py`.

This model is NOT a segmentation model: its output is the per-pixel channel
mean. It exists to validate plumbing, not semantic quality (see ADR-015).
"""

from pathlib import Path


def varint(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def vi(field: int, value: int) -> bytes:
    """A varint field (wire type 0)."""
    return varint(field << 3) + varint(value)


def blob(field: int, payload: bytes) -> bytes:
    """A length-delimited field (wire type 2)."""
    return varint((field << 3) | 2) + varint(len(payload)) + payload


def text(field: int, value: str) -> bytes:
    return blob(field, value.encode("utf-8"))


def dimension(value: int) -> bytes:
    # TensorShapeProto.Dimension { dim_value = 1 }
    return blob(1, vi(1, value))


def tensor_shape(dims) -> bytes:
    return b"".join(dimension(d) for d in dims)


def value_info(name: str, dims) -> bytes:
    # TypeProto.Tensor { elem_type = FLOAT (1), shape }
    tensor = vi(1, 1) + blob(2, tensor_shape(dims))
    type_proto = blob(1, tensor)
    return text(1, name) + blob(2, type_proto)


def attribute_int(name: str, value: int) -> bytes:
    # AttributeProto { name = 1, i = 3, type = 20 (INT = 2) }
    return text(1, name) + vi(3, value) + vi(20, 2)


def attribute_ints(name: str, values) -> bytes:
    # AttributeProto { name = 1, ints = 8, type = 20 (INTS = 7) }
    return text(1, name) + b"".join(vi(8, v) for v in values) + vi(20, 7)


TILE = 512

node = (
    text(1, "input")                              # NodeProto.input
    + text(2, "output")                           # NodeProto.output
    + text(3, "channel_mean")                     # NodeProto.name
    + text(4, "ReduceMean")                       # NodeProto.op_type
    + blob(5, attribute_ints("axes", [1]))        # reduce over channels
    + blob(5, attribute_int("keepdims", 1))
)

graph = (
    blob(1, node)                                 # GraphProto.node
    + text(2, "channel_mean")                     # GraphProto.name
    + blob(11, value_info("input", [1, 3, TILE, TILE]))
    + blob(12, value_info("output", [1, 1, TILE, TILE]))
)

opset = text(1, "") + vi(2, 13)                   # default domain, opset 13

model = (
    vi(1, 8)                                      # ir_version
    + text(2, "luma-probe")                       # producer_name
    + blob(7, graph)
    + blob(8, opset)                              # opset_import
)

out = Path(__file__).with_name("channel_mean.onnx")
out.write_bytes(model)
print(f"wrote {out} ({len(model)} bytes)")
