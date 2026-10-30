<script lang="ts">
  /**
   * SVG Displacement Filter for Liquid Glass Tier 2 Edge Refraction (§8.3)
   * backdrop-filter: url(#frames-lens) blur(2px) saturate(1.6)
   */
</script>

<svg
  id="frames-lens-defs"
  style="position: absolute; width: 0; height: 0; overflow: hidden; pointer-events: none;"
  aria-hidden="true"
>
  <defs>
    <filter id="frames-lens" x="-20%" y="-20%" width="140%" height="140%">
      <!-- Turbulence / displacement generator simulating squircle ramp -->
      <feTurbulence type="fractalNoise" baseFrequency="0.04" numOctaves="2" result="noise" />
      <feDisplacementMap
        in="SourceGraphic"
        in2="noise"
        scale="28"
        xChannelSelector="R"
        yChannelSelector="G"
        result="displaced"
      />
      <feGaussianBlur in="displaced" stdDeviation="1.5" result="blurred" />
      <feColorMatrix
        type="matrix"
        values="1.05 0 0 0 0
                0 1.05 0 0 0
                0 0 1.05 0 0
                0 0 0 1 0"
      />
    </filter>
  </defs>
</svg>
