import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import Counter from './Counter';

describe('Counter', () => {
  it('renders with default initial count of 0', () => {
    render(<Counter />);
    expect(screen.getByText('0')).toBeInTheDocument();
  });

  it('renders with a custom initial count', () => {
    render(<Counter initialCount={5} />);
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('renders with a custom label', () => {
    render(<Counter label="Items" />);
    expect(screen.getByText('Items:')).toBeInTheDocument();
  });

  it('increments count on + click', async () => {
    const user = userEvent.setup();
    render(<Counter />);

    await user.click(screen.getByRole('button', { name: 'Increment' }));
    expect(screen.getByText('1')).toBeInTheDocument();
  });

  it('decrements count on − click', async () => {
    const user = userEvent.setup();
    render(<Counter initialCount={3} />);

    await user.click(screen.getByRole('button', { name: 'Decrement' }));
    expect(screen.getByText('2')).toBeInTheDocument();
  });
});
