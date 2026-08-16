import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { createPortal } from "react-dom";

interface DropdownMenuProps {
  label: ReactNode;
  children: ReactNode;
  className?: string;
  triggerClassName?: string;
  menuClassName?: string;
  menuWidth?: number;
  align?: "start" | "end";
  disabled?: boolean;
}

export function DropdownMenu({
  label,
  children,
  className = "",
  triggerClassName = "",
  menuClassName = "",
  menuWidth = 180,
  align = "start",
  disabled = false,
}: DropdownMenuProps) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });

  const updatePosition = () => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return;
    const padding = 8;
    const left = align === "end" ? rect.right - menuWidth : rect.left;
    setPosition({
      top: rect.bottom + 6,
      left: Math.max(padding, Math.min(left, window.innerWidth - menuWidth - padding)),
    });
  };

  useEffect(() => {
    if (!open) return;
    updatePosition();
    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpen(false);
      triggerRef.current?.focus();
    };
    const reposition = () => updatePosition();
    window.addEventListener("pointerdown", closeOnOutsidePointer);
    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutsidePointer);
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [align, menuWidth, open]);

  const menuStyle: CSSProperties = { top: position.top, left: position.left, width: menuWidth };

  return (
    <div className={`dropdown-menu ${className}`.trim()}>
      <button
        ref={triggerRef}
        type="button"
        className={`dropdown-menu-trigger ${triggerClassName}`.trim()}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          if (!open) updatePosition();
          setOpen((current) => !current);
        }}
      >
        {label}
      </button>
      {open && typeof document !== "undefined" && createPortal(
        <div
          ref={menuRef}
          className={`dropdown-menu-content ${menuClassName}`.trim()}
          role="menu"
          style={menuStyle}
          onClick={(event) => {
            const target = event.target as HTMLElement;
            if (target.closest("button:not(:disabled)")) setOpen(false);
          }}
        >
          {children}
        </div>,
        document.body
      )}
    </div>
  );
}
