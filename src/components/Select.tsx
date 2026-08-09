import { Check, ChevronDown } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { createPortal } from "react-dom";

export type SelectOption = {
  value: string;
  label: string;
  disabled?: boolean;
};

interface SelectProps {
  id?: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
  "aria-label"?: string;
}

export function Select({
  id,
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "请选择",
  className = "",
  "aria-label": ariaLabel,
}: SelectProps) {
  const generatedId = useId();
  const controlId = id ?? `select-${generatedId}`;
  const listboxId = `${controlId}-listbox`;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [position, setPosition] = useState({ top: 0, left: 0, width: 0 });
  const selectedIndex = options.findIndex((option) => option.value === value);
  const selected = options[selectedIndex];

  const updatePosition = () => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return;
    const viewportPadding = 8;
    const maxWidth = window.innerWidth - viewportPadding * 2;
    setPosition({
      top: Math.min(rect.bottom + 4, window.innerHeight - viewportPadding),
      left: Math.max(viewportPadding, Math.min(rect.left, window.innerWidth - Math.min(rect.width, maxWidth) - viewportPadding)),
      width: Math.min(rect.width, maxWidth),
    });
  };

  useEffect(() => {
    if (!open) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const closeOnScroll = () => setOpen(false);
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
      }
    };
    window.addEventListener("pointerdown", closeOnOutsidePointer);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", closeOnScroll, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutsidePointer);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", closeOnScroll, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  const openMenu = () => {
    if (disabled) return;
    updatePosition();
    setActiveIndex(selectedIndex >= 0 ? selectedIndex : options.findIndex((option) => !option.disabled));
    setOpen(true);
  };

  const selectOption = (option: SelectOption) => {
    if (option.disabled) return;
    onChange(option.value);
    setOpen(false);
    triggerRef.current?.focus();
  };

  const moveActive = (direction: 1 | -1) => {
    if (options.length === 0) return;
    let next = activeIndex < 0 ? 0 : activeIndex;
    for (let attempts = 0; attempts < options.length; attempts += 1) {
      next = (next + direction + options.length) % options.length;
      if (!options[next]?.disabled) {
        setActiveIndex(next);
        return;
      }
    }
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) openMenu();
      else moveActive(event.key === "ArrowDown" ? 1 : -1);
      return;
    }
    if (event.key === "Enter" || event.key === " " ) {
      event.preventDefault();
      if (open && activeIndex >= 0) selectOption(options[activeIndex]);
      else openMenu();
    }
  };

  return (
    <div className={`select-control ${className}`}>
      <button
        ref={triggerRef}
        id={controlId}
        type="button"
        className="select-trigger"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-controls={open ? listboxId : undefined}
        aria-expanded={open}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : openMenu())}
        onKeyDown={handleKeyDown}
      >
        <span className={selected ? "" : "select-placeholder"}>{selected?.label ?? placeholder}</span>
        <ChevronDown size={16} aria-hidden="true" className={open ? "select-chevron open" : "select-chevron"} />
      </button>
      {open && typeof document !== "undefined" && createPortal(
        <div
          ref={menuRef}
          id={listboxId}
          className="select-menu"
          role="listbox"
          aria-label={ariaLabel}
          style={{ top: position.top, left: position.left, width: position.width }}
        >
          {options.map((option, index) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              disabled={option.disabled}
              className={[
                "select-option",
                option.value === value ? "selected" : "",
                index === activeIndex ? "active" : "",
              ].filter(Boolean).join(" ")}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => selectOption(option)}
            >
              <span>{option.label}</span>
              {option.value === value && <Check size={14} aria-hidden="true" />}
            </button>
          ))}
        </div>,
        document.body
      )}
    </div>
  );
}
