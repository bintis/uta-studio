import { invoke } from "./runtime";

export type ApiCapability = {
  area: string;
  command: string;
  access: "read" | "mutation" | "destructive" | "external" | "temporary";
  automatedCheck: boolean;
  description: string;
};

export type DiagnosticCheck = {
  id: string;
  status: "passed" | "failed" | "skipped";
  detail: string;
  elapsedMs: number;
};

export type DiagnosticReport = {
  ok: boolean;
  generatedAtMs: number;
  capabilities: number;
  passed: number;
  failed: number;
  skipped: number;
  checks: DiagnosticCheck[];
};

export const getApiCapabilities = (): Promise<ApiCapability[]> =>
  invoke<ApiCapability[]>("api_capabilities");

export const runFeatureDiagnostics = (
  request: { fileHash?: string; includeExportSmoke?: boolean } = {},
): Promise<DiagnosticReport> => invoke<DiagnosticReport>("run_feature_diagnostics", { request });
