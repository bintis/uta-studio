// Design scaffold - compile-shaped reference, not a complete patch.
// Target baseline: ppy.osu.Game 2026.730.0 / uta-ruleset 0.7.2.

using osu.Game.Rulesets.Uta.Scoring;
using osu.Game.Skinning;

namespace osu.Game.Rulesets.Uta.Skinning;

public enum UtaSkinComponents
{
    PitchGuide,
    Lyrics,
    ScoreHud,
    PracticeHud,
    JudgementFeedbackLayer,
    SingingParticleLayer,
    ScoringParticleLayer,
}

public enum UtaCurveRole
{
    Reference,
    Live,
    Trail,
}

public enum UtaGridRole
{
    Major,
    Minor,
}

public enum UtaTargetVisualState
{
    Upcoming,
    Active,
    Completed,
}

public readonly record struct UtaSkinComponentLookup(UtaSkinComponents Component) : ISkinComponentLookup;
public readonly record struct UtaTargetNoteLookup(UtaScoringNoteKind Kind, UtaTargetVisualState State) : ISkinComponentLookup;
public readonly record struct UtaCurveLookup(UtaCurveRole Role) : ISkinComponentLookup;
public readonly record struct UtaGridLookup(UtaGridRole Role) : ISkinComponentLookup;
public readonly record struct UtaScoringFeedbackLookup(UtaNoteGrade Grade, UtaPitchFault Faults) : ISkinComponentLookup;

public enum UtaSkinColour
{
    PitchPanel,
    GridMajor,
    GridMinor,
    TargetNormal,
    TargetGolden,
    TargetFreestyle,
    TargetRap,
    TargetSpoken,
    ReferenceCurve,
    LiveNeutral,
    LiveAccurate,
    LiveNear,
    LiveOff,
    Playhead,
    LyricsCurrent,
    LyricsUpcoming,
    LyricsReading,
    ScorePerfect,
    ScoreGreat,
    ScoreGood,
    ScoreBad,
    ScoreMiss,
}

public enum UtaSkinMetric
{
    PitchGuideHeight,
    HorizontalMargin,
    PlayheadFraction,
    GridMajorWeight,
    GridMinorWeight,
    ReferenceCurveWeight,
    LiveCurveWeight,
    TrailCurveWeight,
    TrailGlow,
    TargetNoteHeight,
    TargetNoteBorder,
    TargetNoteCornerRadius,
    TargetNoteGap,
    LyricLineSpacing,
    HudCornerRadius,
    HudPadding,
}

public enum UtaSkinMotion
{
    AnimationIntensity,
    NotePulseMilliseconds,
    JudgementPopMilliseconds,
    ParticleLifetimeMilliseconds,
    MaximumSingingParticles,
    MaximumScoringParticles,
}
