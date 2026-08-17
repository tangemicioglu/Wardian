import React from "react";
import { Circle, Group, Text } from "react-konva";
import type Konva from "konva";
import {
  CROWN_CAP,
  GLYPH_RADIUS,
  crownPositions,
  type GardenDetail,
  type GardenSkillGlyph,
} from "./skillGlyphs";
import type { GardenTheme } from "./useGardenTheme";

/**
 * Mid lightness on purpose: it lets the monogram be painted in the *background*
 * colour, which reads as white on the light theme and near-black on the dark
 * one. One rule, both themes, no per-theme palette to keep in sync.
 */
function glyphColor(hue: number): string {
  return `hsl(${hue}, 55%, 45%)`;
}

interface SkillCrownProps {
  crown: readonly GardenSkillGlyph[];
  detail: GardenDetail;
  theme: GardenTheme;
  /** entry_ref of the selected skill, so its glyph rings on every carrier. */
  selectedEntryRef: string | null;
  onSelect: (glyph: GardenSkillGlyph) => void;
  onOpen: (glyph: GardenSkillGlyph) => void;
}

/**
 * The ring of skill glyphs around an agent.
 *
 * Provenance is carried by the shape and sync state by the stroke, so the two
 * stay independently readable: a class-inherited skill is ringed whether or not
 * it happened to fall back to a copy.
 */
export const SkillCrown: React.FC<SkillCrownProps> = ({
  crown,
  detail,
  theme,
  selectedEntryRef,
  onSelect,
  onOpen,
}) => {
  const cap = CROWN_CAP[detail];
  if (cap <= 0 || crown.length === 0) return null;

  const shown = crown.slice(0, cap);
  const hidden = crown.length - shown.length;
  // The overflow counter occupies a slot of its own so it cannot land on top of
  // the last glyph.
  const positions = crownPositions(shown.length + (hidden > 0 ? 1 : 0));

  // A click on a glyph must not also select the agent underneath it.
  const stop = (event: Konva.KonvaEventObject<MouseEvent | TouchEvent>) => {
    event.cancelBubble = true;
  };

  return (
    <Group listening>
      {shown.map((glyph, index) => {
        const color = glyphColor(glyph.hue);
        const filled = glyph.provenance !== "global";
        const selected = selectedEntryRef !== null &&
          glyph.entryRef.toLowerCase() === selectedEntryRef.toLowerCase();
        return (
          <Group
            key={glyph.entryRef}
            x={positions[index].x}
            y={positions[index].y}
            onMouseEnter={(event) => {
              event.target.getStage()?.container().style.setProperty("cursor", "pointer");
            }}
            onMouseLeave={(event) => {
              event.target.getStage()?.container().style.setProperty("cursor", "default");
            }}
            onClick={(event) => {
              stop(event);
              onSelect(glyph);
            }}
            onTap={(event) => {
              stop(event);
              onSelect(glyph);
            }}
            onDblClick={(event) => {
              stop(event);
              onOpen(glyph);
            }}
            onDblTap={(event) => {
              stop(event);
              onOpen(glyph);
            }}
          >
            {selected && (
              <Circle radius={GLYPH_RADIUS + 3.5} stroke={theme.selection} strokeWidth={1.5} />
            )}
            {/* An inherited skill gets a second ring: it belongs to the agent's
                class, so it can change without the agent being touched. */}
            {glyph.provenance === "class" && (
              <Circle radius={GLYPH_RADIUS + 2} stroke={color} strokeWidth={1} opacity={0.7} />
            )}
            <Circle
              radius={GLYPH_RADIUS}
              fill={filled ? color : theme.labelBackdrop}
              stroke={color}
              strokeWidth={1}
              // A copy is a fork whose edits never sync back to the library.
              dash={glyph.copied ? [2, 2] : undefined}
            />
            <Text
              text={glyph.monogram}
              fontSize={8}
              fontStyle="bold"
              fontFamily={theme.font}
              fill={filled ? theme.labelBackdrop : color}
              width={GLYPH_RADIUS * 4}
              offsetX={GLYPH_RADIUS * 2}
              offsetY={4}
              align="center"
              listening={false}
            />
          </Group>
        );
      })}
      {hidden > 0 && (
        <Text
          text={`+${hidden}`}
          x={positions[positions.length - 1].x}
          y={positions[positions.length - 1].y}
          fontSize={theme.subLabelSize}
          fontFamily={theme.font}
          fill={theme.labelMuted}
          width={GLYPH_RADIUS * 4}
          offsetX={GLYPH_RADIUS * 2}
          offsetY={5}
          align="center"
          listening={false}
        />
      )}
    </Group>
  );
};
