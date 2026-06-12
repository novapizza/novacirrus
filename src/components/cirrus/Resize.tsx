import { PanelResizeHandle } from "react-resizable-panels";

/**
 * Resize handle. v3 API: PanelResizeHandle renders a div you can style.
 * 4px wide hairline that turns blue on hover/drag.
 */
export function VHandle() {
  return (
    <PanelResizeHandle
      className="
        group/handle relative w-1 shrink-0
        cursor-col-resize bg-border/60
        transition-colors duration-150
        hover:bg-primary/60
        data-[resize-handle-state=hover]:bg-primary/60
        data-[resize-handle-state=drag]:bg-primary
      "
    />
  );
}

export function HHandle() {
  return (
    <PanelResizeHandle
      className="
        group/handle relative h-1 shrink-0
        cursor-row-resize bg-border/60
        transition-colors duration-150
        hover:bg-primary/60
        data-[resize-handle-state=hover]:bg-primary/60
        data-[resize-handle-state=drag]:bg-primary
      "
    />
  );
}
