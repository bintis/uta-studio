"""Typed errors for the offline audio-processing platform."""

from __future__ import annotations


class AudioProcessingError(RuntimeError):
    kind = "audio_processing"

    def __init__(
        self,
        message: str,
        *,
        node_id: str | None = None,
        step_id: str | None = None,
        model_id: str | None = None,
        requested_backend: str | None = None,
        actual_backend: str | None = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.node_id = node_id
        self.step_id = step_id
        self.model_id = model_id
        self.requested_backend = requested_backend
        self.actual_backend = actual_backend

    def to_payload(self) -> dict[str, object]:
        payload: dict[str, object] = {
            "type": "error",
            "kind": self.kind,
            "message": self.message,
        }
        if self.node_id is not None:
            payload["node_id"] = self.node_id
        if self.step_id is not None:
            payload["step_id"] = self.step_id
        if self.model_id is not None:
            payload["model_id"] = self.model_id
        if self.requested_backend is not None:
            payload["requested_backend"] = self.requested_backend
        if self.actual_backend is not None:
            payload["actual_backend"] = self.actual_backend
        return payload


class ModelNotInstalledError(AudioProcessingError):
    kind = "model_not_installed"


class ModelIntegrityError(AudioProcessingError):
    kind = "model_integrity"


class ModelConfigurationError(AudioProcessingError):
    kind = "model_configuration"


class ParameterValidationError(AudioProcessingError):
    kind = "parameter_validation"


class BackendUnavailableError(AudioProcessingError):
    kind = "backend_unavailable"


class InferenceOutOfMemoryError(AudioProcessingError):
    kind = "inference_oom"


class OutputContractError(AudioProcessingError):
    kind = "output_contract"


class CatalogError(AudioProcessingError):
    kind = "catalog"
