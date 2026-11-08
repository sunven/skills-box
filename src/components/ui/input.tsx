import * as React from "react";
import { cn } from "../../lib/utils";

const Input = React.forwardRef<HTMLInputElement, React.ComponentProps<"input">>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        spellCheck={false}
        autoCorrect="off"
        autoComplete="off"
        autoCapitalize="none"
        className={cn(
          "h-9 w-full rounded-md border border-border-strong bg-card px-2.5 text-[13px] text-foreground transition-colors placeholder:text-faint-foreground focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring disabled:cursor-not-allowed disabled:bg-raised disabled:opacity-60",
          className,
        )}
        ref={ref}
        {...props}
      />
    );
  },
);
Input.displayName = "Input";

export { Input };
