import { useState } from 'react';

interface CounterProps {
  initialCount?: number;
  label?: string;
}

export default function Counter({ initialCount = 0, label = 'Count' }: CounterProps) {
  const [count, setCount] = useState(initialCount);

  return (
    <div className="flex items-center gap-4 border border-primary bg-background p-4">
      <span className="text-sm font-medium text-on-surface-variant">{label}:</span>
      <button
        type="button"
        onClick={() => setCount((c) => c - 1)}
        className="border border-primary bg-surface-container px-3 py-1 text-sm font-medium text-on-surface hover:bg-surface-container-high focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        aria-label="Decrement"
      >
        −
      </button>
      <span className="min-w-[2ch] text-center text-lg font-semibold tabular-nums font-data">
        {count}
      </span>
      <button
        type="button"
        onClick={() => setCount((c) => c + 1)}
        className="border border-primary bg-surface-container px-3 py-1 text-sm font-medium text-on-surface hover:bg-surface-container-high focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        aria-label="Increment"
      >
        +
      </button>
    </div>
  );
}
