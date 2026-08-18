namespace osu.Game.Rulesets.Uta.Skinning;

public static class UtaSkinAssetNames
{
    public const string Marker = "uta-skin-marker";
    public const string PitchPanel = "uta-pitch-panel";
    public const string Playhead = "uta-playhead";
    public const string GridMajor = "uta-grid-major";
    public const string GridMinor = "uta-grid-minor";
    public const string CurveReference = "uta-curve-reference";
    public const string CurveLive = "uta-curve-live";
    public const string CurveTrail = "uta-curve-trail";
    public const string HudPanel = "uta-hud-panel";
    public const string HudAccent = "uta-hud-accent";

    public static string Target(string kind) => $"uta-target-note-{kind}";
    public static string Feedback(string grade) => $"uta-feedback-{grade}";
    public static string Fault(string fault) => $"uta-fault-{fault}";
}
