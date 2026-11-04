import { useEffect, useRef } from "react";

export function SelectionCheckbox({
  checked,
  mixed = false,
  disabled = false,
  label,
  onChange,
}: {
  checked: boolean;
  mixed?: boolean;
  disabled?: boolean;
  label: string;
  onChange: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (input.current) input.current.indeterminate = mixed;
  }, [mixed]);
  return (
    <input
      ref={input}
      type="checkbox"
      className="skill-checkbox"
      checked={checked}
      aria-label={label}
      aria-checked={mixed ? "mixed" : checked}
      disabled={disabled}
      onChange={onChange}
    />
  );
}
