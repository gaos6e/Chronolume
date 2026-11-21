import { useId, useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent, PointerEvent, ReactNode, RefObject } from 'react';
import { MoveHorizontal, MoveVertical } from 'lucide-react';
import { useTranslation } from 'react-i18next';

interface DragScrollProps {
  axis: 'x' | 'y';
  label: string;
  className?: string;
  scrollRef?: RefObject<HTMLDivElement | null>;
  children: ReactNode;
}

export function DragScroll({ axis, label, className, scrollRef, children }: DragScrollProps) {
  const { t } = useTranslation();
  const hintId = useId();
  const localRef = useRef<HTMLDivElement>(null);
  const viewport = scrollRef ?? localRef;
  const drag = useRef<{ id: number; position: number; offset: number } | null>(null);
  const [overflowing, setOverflowing] = useState(false);
  const horizontal = axis === 'x';
  const offset = horizontal ? 'scrollLeft' : 'scrollTop';
  const size = horizontal ? 'clientWidth' : 'clientHeight';
  const extent = horizontal ? 'scrollWidth' : 'scrollHeight';
  const position = horizontal ? 'clientX' : 'clientY';
  const HintIcon = horizontal ? MoveHorizontal : MoveVertical;

  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const measure = () => setOverflowing(element[extent] > element[size]);
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    if (element.firstElementChild) observer.observe(element.firstElementChild);
    return () => observer.disconnect();
  }, [extent, size, viewport]);

  const beginDrag = (event: PointerEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    if (!overflowing || event.pointerType === 'touch' || event.button !== 0 || drag.current) return;
    drag.current = { id: event.pointerId, position: event[position], offset: element[offset] };
    element.setPointerCapture(event.pointerId);
    element.dataset.dragging = 'true';
  };

  const moveDrag = (event: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (start?.id !== event.pointerId) return;
    event.currentTarget[offset] = start.offset - (event[position] - start.position);
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== event.pointerId) return;
    drag.current = null;
    delete event.currentTarget.dataset.dragging;
  };

  const navigate = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!overflowing || event.target !== event.currentTarget) return;
    const element = event.currentTarget;
    const step = Math.max(80, element[size] * .7);
    switch (event.key) {
      case horizontal ? 'ArrowLeft' : 'ArrowUp':
      case 'PageUp':
        element[offset] -= step;
        break;
      case horizontal ? 'ArrowRight' : 'ArrowDown':
      case 'PageDown':
        element[offset] += step;
        break;
      case 'Home':
        element[offset] = 0;
        break;
      case 'End':
        element[offset] = element[extent];
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  return <div className="drag-scroll-block">
    <div
      ref={viewport}
      className={`drag-scroll${className ? ` ${className}` : ''}`}
      data-axis={axis}
      data-overflowing={overflowing || undefined}
      role="region"
      tabIndex={overflowing ? 0 : undefined}
      aria-label={label}
      aria-describedby={overflowing ? hintId : undefined}
      onPointerDown={beginDrag}
      onPointerMove={moveDrag}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onLostPointerCapture={endDrag}
      onKeyDown={navigate}
    >
      {children}
    </div>
    {overflowing && <p className="drag-scroll-hint" id={hintId}>
      <HintIcon aria-hidden="true" />
      {t(horizontal ? '左右拖动查看' : '上下拖动查看')}
    </p>}
  </div>;
}
