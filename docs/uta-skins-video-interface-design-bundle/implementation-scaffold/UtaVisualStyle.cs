using osuTK.Graphics;

namespace osu.Game.Rulesets.Uta.Skinning;

/// <summary>
/// Immutable snapshot resolved once per selected skin. Do not perform ISkin lookups in hot paths.
/// </summary>
public sealed record UtaVisualStyle
{
    public required Color4 PitchPanel { get; init; }
    public required Color4 TargetNormal { get; init; }
    public required Color4 ReferenceCurve { get; init; }
    public required Color4 LiveCurve { get; init; }
    public required Color4 Playhead { get; init; }
    public required float ReferenceCurveWeight { get; init; }
    public required float LiveCurveWeight { get; init; }
    public required float TargetNoteHeight { get; init; }
    public required float AnimationIntensity { get; init; }
    public required bool ReducedMotion { get; init; }
}
