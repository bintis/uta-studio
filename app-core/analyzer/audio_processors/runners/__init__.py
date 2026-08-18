from .demucs_torch import DemucsTorchRunner
from .mdx_onnx import MdxOnnxRunner
from .mdxc_torch import MdxcTorchRunner

RUNNERS = {
    "mdxc_torch": MdxcTorchRunner(),
    "mdx_onnx": MdxOnnxRunner(),
    "demucs_torch": DemucsTorchRunner(),
}

__all__ = ["RUNNERS", "DemucsTorchRunner", "MdxOnnxRunner", "MdxcTorchRunner"]
