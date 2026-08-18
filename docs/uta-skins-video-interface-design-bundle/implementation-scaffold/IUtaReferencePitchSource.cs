namespace osu.Game.Rulesets.Uta.Core;

public readonly record struct UtaReferencePitchSample(
    double SongTimeMilliseconds,
    double? Hertz,
    double Confidence,
    bool Voiced);

public interface IUtaReferencePitchSource
{
    bool HasFrameLevelReference { get; }
    bool TrySample(double songTimeMilliseconds, out UtaReferencePitchSample sample);
    void Reset(double songTimeMilliseconds);
}

// Production Auto should generate deterministic 20 ms song-time frames and submit them through
// a synthetic scoring entry point. It must not depend on render frame rate or wall-clock capture
// timestamps. On seek/loop it resets its cursor and uses the scoring controller's new timeline epoch.
