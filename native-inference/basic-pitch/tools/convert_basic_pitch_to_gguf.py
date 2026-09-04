#!/usr/bin/env python3
"""Convert Spotify Basic Pitch ONNX model to GGUF format."""
import os
import sys
from pathlib import Path

import numpy as np
import onnx
from onnx import numpy_helper
import gguf

def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <nmp.onnx> <output.gguf>")
        sys.exit(1)

    onnx_path = sys.argv[1]
    out_path = sys.argv[2]

    print(f"Loading ONNX from {onnx_path}...")
    model = onnx.load(onnx_path)

    initializers = {}
    for init in model.graph.initializer:
        arr = numpy_helper.to_array(init)
        if arr.dtype == np.float32:
            initializers[init.name] = arr

    writer = gguf.GGUFWriter(out_path, "basic_pitch")
    writer.add_name("Spotify Basic Pitch")
    writer.add_description("Lightweight onset and contour neural note activation predictor")
    writer.add_uint32("sample_rate", 22050)
    writer.add_uint32("window_samples", 43844)
    writer.add_uint32("fft_hop_samples", 256)
    writer.add_uint32("n_output_frames", 172)
    writer.add_uint32("n_notes", 88)
    writer.add_uint32("n_contours", 264)

    # Clean, deterministic tensor names.
    #
    # NOTE: the original names here ("backbone.conv1/conv2", "head.onset_conv",
    # "head.onset", "head.note") were guessed from ONNX node-name strings and
    # are WRONG for half of these layers. Cross-referencing the actual weight
    # shapes against basic_pitch/models.py's real layer-construction order
    # (see docs/plans or the uta-basic-pitch-worker implementation notes)
    # proves:
    #   - the [5,5,8,32] conv is the ONSET branch's first conv
    #     (ONSET_KERNEL_SIZE_1=(5,5), n_filters_onsets=32, stride (1,3))
    #   - the [39,3,8,8] (i.e. (3,39)) conv is the CONTOUR branch's first conv
    #     (CONTOUR_KERNEL_SIZE_2=(3,39), 8 filters)
    #   - the [7,7,1,32] conv is the NOTES branch's first conv
    #     (NOTES_KERNEL_SIZE_1=(7,7), n_filters_notes=32, stride (1,3), reads
    #     the 1-channel contour output)
    #   - the [3,7,32,1] (i.e. (7,3)) conv is the NOTES branch's final conv
    #     (NOTES_KERNEL_SIZE_2=(7,3), sigmoid) -> the NOTE output
    #   - the [3,3,33,1] conv is the ONSET branch's final conv
    #     (ONSET_KERNEL_SIZE_2=(3,3), sigmoid, in=33=1+32 from
    #     Concatenate([notes_pre, onset_backbone])) -> the ONSET output
    #   - the [5,5,8,1] conv is correctly the CONTOUR branch's final conv
    #     (CONTOUR_KERNEL_SIZE_3=(5,5), sigmoid) -> the CONTOUR output
    # Named here by true role, not by the old (wrong) guessed names.
    tensor_map = {
        "const_fold_opt__707": "onset_conv1.weight",
        "model_1/re_lu_3/Relu;model_1/re_lu_3/Relu;model_1/batch_normalization_3/FusedBatchNormV3;model_1/batch_normalization_3/FusedBatchNormV3;model_1/conv2d_4/BiasAdd/ReadVariableOp;model_1/conv2d_4/BiasAdd/ReadVariableOp;model_1/conv2d_4/BiasAdd;model_1/conv2d_4/BiasAdd;model_1/conv2d_2/Conv2D;model_1/conv2d_2/Conv2D;model_1/conv2d_4/Conv2D;model_1/conv2d_4/Conv2D": "onset_conv1.bias",
        "const_fold_opt__727": "contour_conv1.weight",
        "model_1/re_lu_1/Relu;model_1/re_lu_1/Relu;model_1/batch_normalization_2/FusedBatchNormV3;model_1/batch_normalization_2/FusedBatchNormV3;model_1/conv2d_1/BiasAdd/ReadVariableOp;model_1/conv2d_1/BiasAdd/ReadVariableOp;model_1/conv2d_1/BiasAdd;model_1/conv2d_1/BiasAdd;model_1/conv2d_1/Conv2D;model_1/conv2d_1/Conv2D": "contour_conv1.bias",
        "const_fold_opt__710": "contour_final.weight",
        "model_1/contours-reduced/BiasAdd/ReadVariableOp;model_1/contours-reduced/BiasAdd/ReadVariableOp": "contour_final.bias",
        "const_fold_opt__738": "note_conv1.weight",
        "model_1/conv2d_2/BiasAdd/ReadVariableOp;model_1/conv2d_2/BiasAdd/ReadVariableOp": "note_conv1.bias",
        "const_fold_opt__702": "note_final.weight",
        "model_1/conv2d_3/BiasAdd/ReadVariableOp;model_1/conv2d_3/BiasAdd/ReadVariableOp": "note_final.bias",
        "const_fold_opt__680": "onset_final.weight",
        "model_1/conv2d_5/BiasAdd/ReadVariableOp;model_1/conv2d_5/BiasAdd/ReadVariableOp": "onset_final.bias",
        "const_fold_opt__664": "cqt.conv_real.weight",
        "const_fold_opt__655": "cqt.conv_imag.weight",
        "const_fold_opt__734": "cqt.lowpass.weight",
        "model_1/cq_t2010v2_1/conv1d_25;model_1/cq_t2010v2_1/conv1d_25": "cqt.unused_zero_bias",
        "model_1/cq_t2010v2_1/Sqrt;model_1/cq_t2010v2_1/Sqrt": "cqt.sqrt_lengths",
        # BatchNormalization applied to the CQT/NormalizedLog output, BEFORE
        # HarmonicStacking (basic_pitch/models.py: get_cqt(..., use_batchnorm=True)).
        # This is NOT foldable into a preceding conv (there isn't one between
        # NormalizedLog and this BN), so unlike the backbone convs' BN it stays
        # as explicit Mul+Add ops in the graph -- easy to miss. A from-scratch
        # port that omits it still runs and produces plausible-looking but
        # wrong activations (contour was unaffected since HarmonicStacking's
        # truncation happens after; note/onset compound the error further
        # downstream). Confirmed by diffing intermediate ONNX node outputs.
        "model_1/batch_normalization/FusedBatchNormV3;model_1/batch_normalization/FusedBatchNormV3": "cqt_bn.scale",
        "model_1/batch_normalization/FusedBatchNormV3;model_1/batch_normalization/FusedBatchNormV31": "cqt_bn.shift",
    }

    print(f"Writing {len(tensor_map)} tensors to {out_path}...")
    for onnx_name, gguf_name in tensor_map.items():
        if onnx_name not in initializers:
            raise ValueError(f"Missing expected initializer: {onnx_name}")
        arr = initializers[onnx_name]
        writer.add_tensor(gguf_name, arr)
        print(f"  {gguf_name}: shape={arr.shape}")

    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_tensors_to_file()
    writer.close()
    size = os.path.getsize(out_path)
    print(f"Successfully generated {out_path} ({size} bytes, {len(tensor_map)} tensors)")

if __name__ == "__main__":
    main()
