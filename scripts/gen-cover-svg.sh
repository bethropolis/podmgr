#!/usr/bin/env bash
set -euo pipefail

# podbox cover SVG generator (Unicode-Safe Vector Compiler)
# Reads raw ASCII art from standard input, outputs a perfectly centered, high-fidelity cover.

python3 - << 'EOF'
import sys

# Grid cell configuration for the wordmark
CW = 6.0         # Character cell width (pixels)
CH = 8.0         # Character cell height (pixels)
PAD_X = 80.0     # Horizontal margin
PAD_Y = 184.0    # Vertical start (centers the wordmark vertically in the 600px canvas)

# Read raw ASCII art from standard input
lines = [line.rstrip('\r\n') for line in sys.stdin]

W = 1200
H = 600

# SVG Header and Background Definitions
svg_header = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="100%" height="auto">
  <defs>
    <!-- Seamless diagonal brand gradient across elements -->
    <linearGradient id="g" x1="80" y1="130" x2="1090" y2="470" gradientUnits="userSpaceOnUse">
      <stop offset="0%"   stop-color="#3b82f6"/>
      <stop offset="25%"  stop-color="#6366f1"/>
      <stop offset="50%"  stop-color="#8b5cf6"/>
      <stop offset="75%"  stop-color="#d946ef"/>
      <stop offset="100%" stop-color="#ec4899"/>
    </linearGradient>

    <!-- Ambient glowing backgrounds -->
    <radialGradient id="glow-left" cx="300" cy="300" r="400" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#6366f1" stop-opacity="0.15"/>
      <stop offset="100%" stop-color="#080b11" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="glow-right" cx="920" cy="300" r="300" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#d946ef" stop-opacity="0.12"/>
      <stop offset="100%" stop-color="#080b11" stop-opacity="0"/>
    </radialGradient>

    <!-- Custom capsule mask mapped to the right-hand icon coordinates -->
    <mask id="split-cover">
      <rect x="0" y="0" width="1200" height="600" fill="#ffffff" />
      <rect x="700" y="291" width="500" height="18" fill="#000000" />
    </mask>
  </defs>

  <!-- Deep space background canvas -->
  <rect width="{W}" height="{H}" fill="#080b11"/>
  
  <!-- Ambient light glow paths -->
  <rect width="{W}" height="{H}" fill="url(#glow-left)"/>
  <rect width="{W}" height="{H}" fill="url(#glow-right)"/>

  <!-- ================= LEFT COLUMN CONTENT ================= -->

  <!-- 1. The Vector-parsed Wordmark -->
  <g>"""

print(svg_header)

# Parse ASCII rows cleanly using Python's native Unicode character tracking
for row, line in enumerate(lines):
    current_char = ""
    start_col = 0
    run_length = 0
    
    Y = round(PAD_Y + row * CH, 2)
    
    # Iterate characters including an extra sentinel at the end to flush the last run
    for col in range(len(line) + 1):
        char = line[col] if col < len(line) else ""
        
        if char != current_char:
            if current_char and current_char != " ":
                X = round(PAD_X + start_col * CW, 2)
                WIDTH = round(run_length * CW, 2)
                
                opacity = ""
                if current_char == "▒":
                    opacity = ' opacity="0.45"'
                elif current_char == "░":
                    opacity = ' opacity="0.25"'
                elif current_char == "▓":
                    opacity = ' opacity="0.75"'
                
                print(f'    <rect x="{X}" y="{Y}" width="{WIDTH}" height="{CH}" fill="url(#g)"{opacity} />')
            
            current_char = char
            start_col = col
            run_length = 1
        else:
            run_length += 1

# SVG Footer and structural Layout Elements
svg_footer = f"""  </g>

  <!-- 2. Tagline (system-sans, italicized, balanced spacing) -->
  <text x="80" y="325" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, sans-serif" font-size="22" font-style="italic" font-weight="500" fill="#94a3b8" letter-spacing="-0.5px">Define once. Run anywhere. No daemon.</text>

  <!-- 3. Glassmorphism Feature Chips (Y=375) -->
  <!-- Chip 1: Podman Native -->
  <g transform="translate(80, 375)">
    <rect x="0" y="0" width="130" height="36" rx="18" fill="rgba(30, 41, 59, 0.4)" stroke="rgba(59, 130, 246, 0.25)" stroke-width="1.5" />
    <circle cx="16" cy="18" r="4" fill="#3b82f6" />
    <text x="28" y="22" font-family="-apple-system, BlinkMacSystemFont, sans-serif" font-size="12" font-weight="600" fill="#e2e8f0" text-anchor="start">Podman Native</text>
  </g>

  <!-- Chip 2: systemd Managed -->
  <g transform="translate(222, 375)">
    <rect x="0" y="0" width="145" height="36" rx="18" fill="rgba(30, 41, 59, 0.4)" stroke="rgba(99, 102, 241, 0.25)" stroke-width="1.5" />
    <circle cx="16" cy="18" r="4" fill="#6366f1" />
    <text x="28" y="22" font-family="-apple-system, BlinkMacSystemFont, sans-serif" font-size="12" font-weight="600" fill="#e2e8f0" text-anchor="start">systemd Managed</text>
  </g>

  <!-- Chip 3: Wayland & Audio -->
  <g transform="translate(379, 375)">
    <rect x="0" y="0" width="140" height="36" rx="18" fill="rgba(30, 41, 59, 0.4)" stroke="rgba(139, 92, 246, 0.25)" stroke-width="1.5" />
    <circle cx="16" cy="18" r="4" fill="#8b5cf6" />
    <text x="28" y="22" font-family="-apple-system, BlinkMacSystemFont, sans-serif" font-size="12" font-weight="600" fill="#e2e8f0" text-anchor="start">Wayland &amp; Audio</text>
  </g>

  <!-- Chip 4: Strict Sandbox -->
  <g transform="translate(531, 375)">
    <rect x="0" y="0" width="130" height="36" rx="18" fill="rgba(30, 41, 59, 0.4)" stroke="rgba(217, 70, 239, 0.25)" stroke-width="1.5" />
    <circle cx="16" cy="18" r="4" fill="#d946ef" />
    <text x="28" y="22" font-family="-apple-system, BlinkMacSystemFont, sans-serif" font-size="12" font-weight="600" fill="#e2e8f0" text-anchor="start">Strict Sandbox</text>
  </g>


  <!-- ================= RIGHT COLUMN BRAND ICON ================= -->
  <!-- Centered vertically in the 600px height at Y=130 -->
  <g>
    <!-- Outer Squircle Frame (Width=340, Height=340, Stroke=40) -->
    <rect x="750" y="130" width="340" height="340" rx="80" fill="none" stroke="url(#g)" stroke-width="40" />
    
    <!-- Centered Inner Pod split capsule (Width=92, Height=184, split-mask applied) -->
    <rect x="874" y="208" width="92" height="184" rx="46" fill="url(#g)" mask="url(#split-cover)" />
  </g>

</svg>"""

print(svg_footer)
EOF
