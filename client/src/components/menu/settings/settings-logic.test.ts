import assert from "node:assert/strict";
import test from "node:test";
import { getSettingsStops } from "./constants.ts";
import { clampModelSettingNumber, modelSettingsVisibility } from "./settings-logic.ts";

test("bounded model values accept direct input and clamp invalid ranges", () => {
  assert.equal(clampModelSettingNumber("7", 8), 7);
  assert.equal(clampModelSettingNumber("7.6", 8), 8);
  assert.equal(clampModelSettingNumber("", 8), 8);
  assert.equal(clampModelSettingNumber("invalid", 8), 8);
  assert.equal(clampModelSettingNumber(-5, 8), 1);
  assert.equal(clampModelSettingNumber(99, 8), 16);
});

test("switching transcription engines preserves shared and fallback settings", () => {
  assert.deepEqual(modelSettingsVisibility("whisper"), {
    whisperModel: true,
    whisperSearch: true,
    batchSize: true,
    wordAlignment: true,
    pitchModel: true,
    activeEngine: "whisper",
  });
  assert.deepEqual(modelSettingsVisibility("parakeet"), {
    whisperModel: true,
    whisperSearch: true,
    batchSize: true,
    wordAlignment: true,
    pitchModel: true,
    activeEngine: "parakeet",
  });
  assert.deepEqual(getSettingsStops("models", true, 0, 2), [4, 1, 1, 1, 1, 1]);
  assert.deepEqual(getSettingsStops("analysis", false), [4, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1]);
});
