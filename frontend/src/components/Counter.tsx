import { useState } from 'react';

interface CounterProps {
  initialCount?: number;
  label?: string;
}

export default function Counter({ initialCount = 0, label = 'Count' }: CounterProps) {
  const [count, setCount] = useState(initialCount);

  return (
    <div className="flex items-center gap-4 rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
      <span className="text-sm font-medium text-gray-700">{label}:</span>
      <button
        type="button"
        onClick={() => setCount((c) => c - 1)}
        className="rounded bg-gray-200 px-3 py-1 text-sm font-medium hover:bg-gray-300"
        aria-label="Decrement"
      >
        −
      </button>
      <span className="min-w-[2ch] text-center text-lg font-semibold tabular-nums">
        {count}
      </span>
      <button
        type="button"
        onClick={() => setCount((c) => c + 1)}
        className="rounded bg-gray-200 px-3 py-1 text-sm font-medium hover:bg-gray-300"
        aria-label="Increment"
      >
        +
      </button>
    </div>
  );
}
