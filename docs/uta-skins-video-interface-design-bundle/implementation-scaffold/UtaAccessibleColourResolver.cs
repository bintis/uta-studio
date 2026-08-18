using System;
using osuTK.Graphics;

namespace osu.Game.Rulesets.Uta.Skinning;

public static class UtaAccessibleColourResolver
{
    public static double ContrastRatio(Color4 a, Color4 b)
    {
        static double channel(float value)
            => value <= 0.04045f ? value / 12.92 : Math.Pow((value + 0.055) / 1.055, 2.4);

        static double luminance(Color4 c)
            => 0.2126 * channel(c.R) + 0.7152 * channel(c.G) + 0.0722 * channel(c.B);

        double l1 = luminance(a);
        double l2 = luminance(b);
        return (Math.Max(l1, l2) + 0.05) / (Math.Min(l1, l2) + 0.05);
    }

    /// <summary>
    /// Preserve the requested hue where possible, but return a safe fallback when contrast is
    /// insufficient. Critical elements must also retain shape/pattern redundancy.
    /// </summary>
    public static Color4 EnsureContrast(Color4 requested, Color4 background, Color4 fallback, double minimum = 3.0)
        => ContrastRatio(requested, background) >= minimum ? requested : fallback;
}
