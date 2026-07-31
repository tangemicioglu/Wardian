import React from "react";
import { Circle, Group, Text } from "react-konva";
import type { GardenAgentUnit } from "./garden.types";
import { gardenAgentStatusColor, isActiveAgentStatus } from "./gardenStatus";
import { resolveCssVar } from "./resolveColor";
import { SkillCrown } from "./SkillCrown";
import type { GardenDetail, GardenSkillGlyph } from "./skillGlyphs";
import { useGardenPulse } from "./useGardenPulse";
import type { GardenTheme } from "./useGardenTheme";

/** Konva node `name` used to identify agent units during canvas hit-testing. */
export const AGENT_UNIT_NAME = "agent-unit";

interface AgentUnitProps {
  unit: GardenAgentUnit;
  selected: boolean;
  /**
   * True when a skill is selected and this agent carries it.
   *
   * Instancing a skill across its carriers removes the ability to point at
   * *the* place it lives, so the reverse highlight has to answer the same
   * question with a set. Distinct from `selected`, which is a single unit.
   */
  highlighted?: boolean;
  detail: GardenDetail;
  theme: GardenTheme;
  /** entry_ref of the selected skill, so its glyph rings on every carrier. */
  selectedSkillRef?: string | null;
  onSelect: (id: string) => void;
  onOpen: (id: string) => void;
  onSelectSkill: (glyph: GardenSkillGlyph) => void;
  onOpenSkill: (glyph: GardenSkillGlyph) => void;
  onDragMove: (x: number, y: number) => void;
}

export const AgentUnit: React.FC<AgentUnitProps> = ({
  unit,
  selected,
  highlighted = false,
  detail,
  theme,
  selectedSkillRef = null,
  onSelect,
  onOpen,
  onSelectSkill,
  onOpenSkill,
  onDragMove,
}) => {
  const fill = resolveCssVar(gardenAgentStatusColor(unit.status));
  const pulse = useGardenPulse(isActiveAgentStatus(unit.status));

  return (
    <Group
      x={unit.position.x}
      y={unit.position.y}
      // id + name let the canvas resolve which agent was right-clicked via
      // Konva hit-testing (see GardenCanvas), without per-node DOM handlers.
      id={unit.ref.id}
      name={AGENT_UNIT_NAME}
      draggable
      onClick={() => onSelect(unit.ref.id)}
      onTap={() => onSelect(unit.ref.id)}
      onDblClick={() => onOpen(unit.ref.id)}
      onDblTap={() => onOpen(unit.ref.id)}
      onDragMove={(e) => onDragMove(e.target.x(), e.target.y())}
    >
      <Circle radius={18 * pulse} fill={fill} opacity={0.18} />
      {/* Drawn outside the status halo so a carrier stands out without
          overriding the status colour, which stays the primary channel. */}
      {highlighted && (
        <Circle radius={21} stroke={theme.selection} strokeWidth={2} opacity={0.85} dash={[3, 3]} />
      )}
      <Circle
        radius={11}
        fill={fill}
        stroke={selected ? theme.selection : "transparent"}
        strokeWidth={2}
      />
      <SkillCrown
        crown={unit.crown}
        detail={detail}
        theme={theme}
        selectedEntryRef={selectedSkillRef}
        onSelect={onSelectSkill}
        onOpen={onOpenSkill}
      />
      <Text
        text={unit.label}
        fontSize={theme.labelSize}
        fontFamily={theme.font}
        fill={theme.label}
        y={24}
        width={140}
        offsetX={70}
        align="center"
        // Halo in the background colour: labels sit over status halos and
        // neighbouring units, and a map label has to stay readable wherever it
        // lands. Cheaper and less cluttered than a backdrop rectangle.
        shadowColor={theme.labelBackdrop}
        shadowBlur={4}
        shadowOpacity={1}
      />
    </Group>
  );
};
