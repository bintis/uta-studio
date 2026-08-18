namespace osu.Game.Rulesets.Uta.Formats;

public enum UtzDiagnosticSeverity
{
    Warning,
    Error,
}

public sealed record UtzDiagnostic(
    string Code,
    UtzDiagnosticSeverity Severity,
    string MessageKey,
    string? PackageRelativePath = null,
    string? RemediationKey = null);

public sealed class UtzValidationException : System.IO.InvalidDataException
{
    public UtzDiagnostic Diagnostic { get; }

    public UtzValidationException(UtzDiagnostic diagnostic, System.Exception? inner = null)
        : base(diagnostic.Code, inner)
    {
        Diagnostic = diagnostic;
    }
}
