"""Uta Studio offline adapter around audio-separator 0.44.5.

This package is the in-repo compatibility layer required by the audio-model
plan: load from already-installed files, honor an explicit device, and never
contact the network during analysis.
"""

from .offline import OfflineSeparator, apply_torch_device, load_model_from_spec

__all__ = [
    "OfflineSeparator",
    "apply_torch_device",
    "load_model_from_spec",
]
