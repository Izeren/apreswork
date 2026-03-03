// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import type { ComponentProps } from 'svelte';
import DurationInput from './DurationInput.svelte';
import DateTimePicker from './DateTimePicker.svelte';
import { parseDuration } from '../../utils';
import { makeTimeMenuGetBCR, TEST_NOW } from '../../testFixtures';

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  cleanup();
});

type DurationProps = ComponentProps<typeof DurationInput>;
type PickerProps = ComponentProps<typeof DateTimePicker>;

/** Render DurationInput with defaults (value 30, spy onchange); returns input + spy. */
function renderDuration(props: Partial<DurationProps> = {}) {
  const onchange = vi.fn();
  const utils = render(DurationInput, { value: 30, onchange, ...props });
  // Query by role so a custom `label` prop doesn't break the lookup.
  const input = utils.getByRole('combobox') as HTMLInputElement;
  return { ...utils, input, onchange };
}

async function renderDurationOpen(props: Partial<DurationProps> = {}) {
  const utils = renderDuration(props);
  await fireEvent.focus(utils.input);
  return utils;
}

async function typeInto(input: HTMLInputElement, text: string) {
  input.value = text;
  await fireEvent.input(input);
}

/** Dropdown option labels with the ✓ committed-value marker stripped. */
function optionLabels(options: HTMLElement[]) {
  return options.map((o) => o.textContent?.trim().replace('✓', '').trim());
}

function renderPicker(props: Partial<PickerProps> & Pick<PickerProps, 'value'>) {
  const onchange = vi.fn();
  const utils = render(DateTimePicker, { onchange, now: TEST_NOW, ...props });
  return { ...utils, onchange };
}

function lastIso(onchange: ReturnType<typeof vi.fn>): string {
  return onchange.mock.calls.at(-1)?.[0] as string;
}

async function clickShortcut(container: HTMLElement, shortcut: string) {
  await fireEvent.click(
    container.querySelector(`button[data-shortcut="${shortcut}"]`) as HTMLButtonElement,
  );
}

async function clickTime(container: HTMLElement, time: string) {
  await fireEvent.click(
    container.querySelector(`.time-menu button[data-time="${time}"]`) as HTMLButtonElement,
  );
}

describe('DurationInput — initial display from numeric value', () => {
  const cases = [
    { value: 90, expected: '1h 30m' },
    { value: 60, expected: '1h' },
    { value: 30, expected: '30m' },
    { value: 0, expected: '0m' },
    { value: 125, expected: '2h 5m' },
  ];

  it.each(cases)(
    'shows "$expected" in the text field for $value minutes',
    ({ value, expected }) => {
      const { input } = renderDuration({ value });
      expect(input.value).toBe(expected);
    },
  );
});

describe('parseDuration — valid inputs', () => {
  const cases = [
    { input: '120m', expected: 120 },
    { input: '2h', expected: 120 },
    { input: '2h 5m', expected: 125 },
    { input: '1h 30m', expected: 90 },
    { input: '1h30m', expected: 90 },
    { input: '15m', expected: 15 },
    { input: '90', expected: 90 },
    { input: '0m', expected: 0 },
    { input: '0h', expected: 0 },
    { input: '0', expected: 0 },
  ];

  it.each(cases)('parses "$input" → $expected minutes', ({ input, expected }) => {
    expect(parseDuration(input)).toBe(expected);
  });
});

describe('parseDuration — case-insensitive and whitespace-tolerant', () => {
  const cases = [
    { input: '2H 30M', expected: 150 },
    { input: '  1h  30m  ', expected: 90 },
    { input: '2H30M', expected: 150 },
    { input: '  60  ', expected: 60 },
  ];

  it.each(cases)('parses "$input" → $expected minutes', ({ input, expected }) => {
    expect(parseDuration(input)).toBe(expected);
  });
});

describe('parseDuration — invalid inputs return null', () => {
  const cases = [
    { input: '' },
    { input: '   ' },
    { input: 'abc' },
    { input: '1h 2h' },
    { input: '30s' },
    { input: 'one hour' },
    { input: '1.5h' },
    { input: '-5' },
    { input: '-1h' },
  ];

  it.each(cases)('returns null for "$input"', ({ input }) => {
    expect(parseDuration(input)).toBeNull();
  });
});

describe('DurationInput — onchange on blur', () => {
  const cases = [
    { typed: '2h 30m', expectedMinutes: 150 },
    { typed: '45m', expectedMinutes: 45 },
    { typed: '1h', expectedMinutes: 60 },
    { typed: '90', expectedMinutes: 90 },
    { typed: '1h30m', expectedMinutes: 90 },
  ];

  it.each(cases)(
    'calls onchange($expectedMinutes) when user types "$typed" and blurs',
    async ({ typed, expectedMinutes }) => {
      const { input, onchange } = renderDuration();
      await typeInto(input, typed);
      await fireEvent.blur(input);
      expect(onchange).toHaveBeenCalledWith(expectedMinutes);
    },
  );
});

describe('DurationInput — onchange on Enter', () => {
  it('calls onchange when user presses Enter after typing a valid duration', async () => {
    const { input, onchange } = renderDuration();
    await typeInto(input, '2h');
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onchange).toHaveBeenCalledWith(120);
  });

  it('does not call onchange when user presses a non-Enter key', async () => {
    const { input, onchange } = renderDuration();
    await typeInto(input, '2h');
    await fireEvent.keyDown(input, { key: 'Tab' });
    expect(onchange).not.toHaveBeenCalled();
  });
});

describe('DurationInput — parse error', () => {
  it('shows a parse error message when the input cannot be parsed on blur', async () => {
    const { input, getByText } = renderDuration();
    await typeInto(input, 'not a duration');
    await fireEvent.blur(input);
    expect(getByText(/invalid format/i)).toBeTruthy();
  });

  it('does not show parse error on initial render with a valid value', () => {
    const { queryByText } = renderDuration();
    expect(queryByText(/invalid format/i)).toBeNull();
  });

  it('does not call onchange when the input is invalid', async () => {
    const { input, onchange } = renderDuration();
    await typeInto(input, 'xyz');
    await fireEvent.blur(input);
    expect(onchange).not.toHaveBeenCalled();
  });

  it('restores previous value when user blurs with blank input', async () => {
    const { input, onchange } = renderDuration();
    await typeInto(input, '');
    await fireEvent.blur(input);
    // Blank blur restores the previous value instead of showing a parse error
    expect(input.value).toBe('30m');
    expect(onchange).not.toHaveBeenCalled();
  });
});

describe('DurationInput — validation message', () => {
  it.each([
    { value: 3, min: 5, shouldShow: true, label: 'when value < min' },
    { value: 5, min: 5, shouldShow: false, label: 'when value equals min' },
    { value: 30, min: 5, shouldShow: false, label: 'when value exceeds min' },
    { value: 0, min: 5, shouldShow: true, label: 'zero-minute value with default min=5' },
  ])('shows or hides below-min message $label', ({ value, min, shouldShow }) => {
    const { queryByText } = renderDuration({ value, min });
    if (shouldShow) {
      expect(queryByText(/minimum duration/i)).toBeTruthy();
    } else {
      expect(queryByText(/minimum duration/i)).toBeNull();
    }
  });

  it('shows below-min message after user enters a valid but too-small value', async () => {
    const { input, getByText } = renderDuration({ min: 10 });
    await typeInto(input, '5m');
    await fireEvent.blur(input);
    expect(getByText(/minimum duration/i)).toBeTruthy();
  });

  // The `min` prop is advisory — the parent form validates on submit.
  // onchange is still called with the parsed value even when it is below min.
  it('calls onchange with the below-min value (min is advisory)', async () => {
    const { input, onchange } = renderDuration({ min: 10 });
    await typeInto(input, '3m');
    await fireEvent.blur(input);
    expect(onchange).toHaveBeenCalledWith(3);
  });
});

describe('DurationInput — label', () => {
  it.each([
    { label: undefined, expected: 'Duration' },
    { label: 'Estimated time', expected: 'Estimated time' },
  ])('renders label "$expected"', ({ label, expected }) => {
    const { getByText } = renderDuration({ label });
    expect(getByText(expected)).toBeTruthy();
  });
});

describe('DurationInput — disabled state', () => {
  it.each([
    { disabled: true, expected: true },
    { disabled: undefined, expected: false },
  ])('input disabled=$expected when disabled=$disabled', ({ disabled, expected }) => {
    const { input } = renderDuration(disabled !== undefined ? { disabled } : {});
    expect(input.disabled).toBe(expected);
  });
});

describe('DurationInput — duration preview', () => {
  const previewCases = [
    { value: 90, expected: '= 1h 30m' },
    { value: 60, expected: '= 1h' },
    { value: 30, expected: '= 30m' },
  ];

  it.each(previewCases)(
    'shows preview "$expected" for $value minutes on initial render',
    ({ value, expected }) => {
      const { getByText } = renderDuration({ value });
      expect(getByText(expected)).toBeTruthy();
    },
  );

  it('normalises the input text to canonical form after a successful parse on blur', async () => {
    const { input } = renderDuration();
    await typeInto(input, '90');
    await fireEvent.blur(input);
    expect(input.value).toBe('1h 30m');
  });
});

describe('DurationInput — autocomplete dropdown', () => {
  it('shows all presets when input is focused', async () => {
    const { getAllByRole } = await renderDurationOpen();
    expect(getAllByRole('option')).toHaveLength(8);
  });

  it('shows no dropdown before the input is focused', () => {
    const { queryAllByRole } = renderDuration();
    expect(queryAllByRole('option')).toHaveLength(0);
  });

  it.each([
    { typed: '1', expectedLabels: ['15 min', '1 hour'] },
    { typed: '2', expectedLabels: ['2 hours'] },
    { typed: '4', expectedLabels: ['45 min', '4 hours'] },
    { typed: 'hour', expectedLabels: ['1 hour', '2 hours', '4 hours', '8 hours'] },
    { typed: 'min', expectedLabels: ['5 min', '15 min', '30 min', '45 min'] },
    { typed: '30', expectedLabels: ['30 min'] },
  ])('filters to "$expectedLabels" when typing "$typed"', async ({ typed, expectedLabels }) => {
    const { input, getAllByRole } = renderDuration();
    await typeInto(input, typed);
    expect(optionLabels(getAllByRole('option'))).toEqual(expectedLabels);
  });

  it('hides dropdown when no presets match the typed text', async () => {
    const { input, queryAllByRole } = renderDuration();
    await typeInto(input, 'xyz99');
    expect(queryAllByRole('option')).toHaveLength(0);
  });

  it('selecting a preset via click calls onchange and updates input text', async () => {
    const { input, getByText, onchange } = await renderDurationOpen();

    await fireEvent.click(getByText('15 min'));

    expect(onchange).toHaveBeenCalledWith(15);
    expect(input.value).toBe('15 min');
  });

  it('ArrowDown moves highlight to next option', async () => {
    const { input, getAllByRole } = await renderDurationOpen();
    await fireEvent.keyDown(input, { key: 'ArrowDown' });

    const options = getAllByRole('option');
    expect(options[0].classList.contains('duration-option--highlighted')).toBe(true);
  });

  it('ArrowUp from no selection wraps to last option', async () => {
    const { input, getAllByRole } = await renderDurationOpen();
    await fireEvent.keyDown(input, { key: 'ArrowUp' });

    const options = getAllByRole('option');
    expect(options[options.length - 1].classList.contains('duration-option--highlighted')).toBe(
      true,
    );
  });

  it('ArrowDown wraps around from last to first option', async () => {
    const { input, getAllByRole } = await renderDurationOpen();
    for (let i = 0; i < 9; i++) {
      await fireEvent.keyDown(input, { key: 'ArrowDown' });
    }

    const options = getAllByRole('option');
    expect(options[0].classList.contains('duration-option--highlighted')).toBe(true);
  });

  it('Enter on highlighted option selects it', async () => {
    const { input, onchange } = await renderDurationOpen();
    await fireEvent.keyDown(input, { key: 'ArrowDown' }); // highlights index 0: 5 min
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(onchange).toHaveBeenCalledWith(5);
    expect(input.value).toBe('5 min');
  });

  it('Enter with no option highlighted commits typed text', async () => {
    const { input, onchange } = await renderDurationOpen();
    await typeInto(input, '2h');
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(onchange).toHaveBeenCalledWith(120);
  });

  it('Escape closes dropdown without committing', async () => {
    const { input, queryAllByRole, onchange } = await renderDurationOpen();
    await fireEvent.keyDown(input, { key: 'Escape' });

    expect(queryAllByRole('option')).toHaveLength(0);
    expect(onchange).not.toHaveBeenCalled();
  });

  it('shows checkmark (✓) on option that matches the current committed value', async () => {
    const { getAllByRole } = await renderDurationOpen();

    const options = getAllByRole('option');
    // value=30 corresponds to "30 min" (index 2 among 8 presets)
    const thirtyMinOption = options.find((o) => o.textContent?.includes('30 min'));
    expect(thirtyMinOption).toBeDefined();
    expect(thirtyMinOption!.textContent).toContain('✓');

    const fifteenMinOption = options.find((o) => o.textContent?.includes('15 min'));
    expect(fifteenMinOption!.textContent).not.toContain('✓');
  });

  it('dropdown closes after selection via click', async () => {
    const { getByText, queryAllByRole } = await renderDurationOpen();

    await fireEvent.click(getByText('1 hour'));

    expect(queryAllByRole('option')).toHaveLength(0);
  });

  it('dropdown closes after Enter on highlighted option', async () => {
    const { input, queryAllByRole } = await renderDurationOpen();
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(queryAllByRole('option')).toHaveLength(0);
  });

  it('input has role="combobox" and aria-expanded reflects dropdown state', async () => {
    const { input } = renderDuration();

    expect(input.getAttribute('role')).toBe('combobox');
    expect(input.getAttribute('aria-expanded')).toBe('false');

    await fireEvent.focus(input);
    expect(input.getAttribute('aria-expanded')).toBe('true');
  });

  it('aria-activedescendant updates as arrow keys move highlight', async () => {
    const { input } = await renderDurationOpen();

    expect(input.getAttribute('aria-activedescendant')).toBeNull();

    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(input.getAttribute('aria-activedescendant')).toBe('duration-option-0');

    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(input.getAttribute('aria-activedescendant')).toBe('duration-option-1');
  });

  it('shows a custom option when typed value is valid but not a preset', async () => {
    const { input, getAllByRole } = renderDuration();
    await typeInto(input, '50');

    // Custom "50m" option at top, no matching presets
    expect(optionLabels(getAllByRole('option'))).toEqual(['50m']);
  });

  it('does not show custom option when typed value matches a preset', async () => {
    const { input, getAllByRole } = renderDuration();
    await typeInto(input, '30');

    // Only the preset, no duplicate custom option
    expect(optionLabels(getAllByRole('option'))).toEqual(['30 min']);
  });

  it('does not show custom option when parsed value is below min', async () => {
    const { input, getAllByRole } = renderDuration({ min: 10 });
    await typeInto(input, '3');

    // "3" parses to 3 minutes (below min=10), so no custom option is shown.
    // But "30 min" preset still appears because "3" matches its label.
    const labels = optionLabels(getAllByRole('option'));
    expect(labels).toEqual(['30 min']);
    expect(labels).not.toContain('3m');
  });

  it('custom option can be selected via click', async () => {
    const { input, getByText, onchange } = renderDuration();
    await typeInto(input, '50');
    await fireEvent.click(getByText('50m'));

    expect(onchange).toHaveBeenCalledWith(50);
    expect(input.value).toBe('50m');
  });

  it('clears the input text on focus', async () => {
    const { input } = await renderDurationOpen({ value: 60 });
    expect(input.value).toBe('');
  });

  it('Escape restores previous value in the input text', async () => {
    const { input } = await renderDurationOpen({ value: 60 });
    // Input was cleared on focus
    expect(input.value).toBe('');
    await fireEvent.keyDown(input, { key: 'Escape' });

    // Should restore the formatted value
    expect(input.value).toBe('1h');
  });
});

// Decodes the ISO string in the browser's local timezone — same coordinate space
// as isoToLocalTime/isoToLocalDate, so the assertions stay consistent with the component.
function expectLocalDateTime(
  iso: string,
  expectedYear: number,
  expectedMonth: number,
  expectedDay: number,
  expectedHours: number,
  expectedMinutes: number,
) {
  const date = new Date(iso);
  expect(date.getFullYear()).toBe(expectedYear);
  expect(date.getMonth()).toBe(expectedMonth - 1);
  expect(date.getDate()).toBe(expectedDay);
  expect(date.getHours()).toBe(expectedHours);
  expect(date.getMinutes()).toBe(expectedMinutes);
}

describe('DateTimePicker — initial rendering from ISO string', () => {
  it('renders the selected date and time in the trigger', () => {
    const { getByRole } = renderPicker({ value: '2026-03-15T12:00:00Z' });
    expect(getByRole('button', { name: /selected 15\/03\/2026 at 12:00/i })).toBeTruthy();
  });

  it('shows placeholder text when value is null', () => {
    const { getByRole } = renderPicker({ value: null });
    const trigger = getByRole('button', { name: /choose date and time/i });
    expect(trigger.textContent).toContain('Pick a date');
  });

  it('uses relative labels for today and tomorrow in the trigger', () => {
    vi.useFakeTimers();
    vi.setSystemTime(TEST_NOW);

    const { getByRole, rerender } = renderPicker({ value: '2026-01-01T12:00:00Z' });
    expect(getByRole('button', { name: /selected today at/i })).toBeTruthy();

    rerender({ value: '2026-01-02T12:00:00Z', onchange: vi.fn() });
    expect(getByRole('button', { name: /selected tomorrow at/i })).toBeTruthy();
  });
});

describe('DateTimePicker — timezone hint', () => {
  it('renders a timezone hint containing UTC', () => {
    const { getByText } = renderPicker({ value: null });
    const hint = getByText(/UTC/i);
    expect(hint).toBeTruthy();
  });
});

describe('DateTimePicker — onchange', () => {
  it('uses the default time when selecting a quick date', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(TEST_NOW);

    const { container, getByRole, onchange } = renderPicker({
      value: null,
      defaultTime: '23:59',
      now: TEST_NOW,
    });
    await fireEvent.click(getByRole('button', { name: /choose date and time/i }));
    await clickShortcut(container, 'today');

    expectLocalDateTime(lastIso(onchange), 2026, 1, 1, 23, 59);
  });

  it('anchors "This week" to Sunday', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(TEST_NOW);

    const { container, getByRole, onchange } = renderPicker({ value: null, defaultTime: '23:59' });
    await fireEvent.click(getByRole('button', { name: /choose date and time/i }));
    await clickShortcut(container, 'this-week');

    expectLocalDateTime(lastIso(onchange), 2026, 1, 4, 23, 59);
  });

  it.each([
    { desc: 'from the dropdown', clickedTime: '23:59', expectedH: 23, expectedM: 59 },
    {
      desc: 'from quick presets inside the time menu',
      clickedTime: '17:00',
      expectedH: 17,
      expectedM: 0,
    },
  ])('updates the selected time $desc', async ({ clickedTime, expectedH, expectedM }) => {
    const { container, getByRole, onchange } = renderPicker({ value: '2026-03-15T12:00:00Z' });
    await fireEvent.click(getByRole('button', { name: /selected 15\/03\/2026 at 12:00/i }));
    await fireEvent.click(getByRole('button', { name: 'Time' }));
    await clickTime(container, clickedTime);

    expectLocalDateTime(lastIso(onchange), 2026, 3, 15, expectedH, expectedM);
  });

  it('shows quick time presets only after opening the time menu', async () => {
    const { container, getByRole } = renderPicker({ value: '2026-03-15T12:00:00Z' });

    await fireEvent.click(getByRole('button', { name: /selected 15\/03\/2026 at 12:00/i }));
    expect(container.querySelector('.time-menu button[data-time="09:00"]')).toBeNull();

    await fireEvent.click(getByRole('button', { name: 'Time' }));
    expect(container.querySelector('.time-menu button[data-time="09:00"]')).toBeTruthy();
  });

  it('scrolls to the selected non-preset time when the time menu opens', async () => {
    const { container, getByRole } = renderPicker({ value: '2026-03-15T20:00:00Z' });

    vi.spyOn(HTMLDivElement.prototype, 'clientHeight', 'get').mockImplementation(function (
      this: HTMLDivElement,
    ) {
      return this.classList.contains('time-menu-list') ? 120 : 0;
    });
    // Stub getBoundingClientRect per element class so the scroll-center formula
    // has realistic positions to work with.
    // relativeTop = optionTop(290) − listTop(50) + scrollTop(0) = 240
    // targetScrollTop = max(0, 240 − (120 − 32) / 2) = max(0, 240 − 44) = 196
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
      makeTimeMenuGetBCR({
        listHeight: 120,
        listBottom: 170,
        activeTop: 290,
        activeBottom: 322,
        activeY: 290,
      }),
    );

    await fireEvent.click(getByRole('button', { name: /selected 15\/03\/2026 at 20:00/i }));
    await fireEvent.click(getByRole('button', { name: 'Time' }));
    await fireEvent.click(getByRole('button', { name: 'Time' }));
    await fireEvent.click(getByRole('button', { name: 'Time' }));
    const list = container.querySelector('.time-menu-list') as HTMLDivElement;

    expect(list.scrollTop).toBe(196);
  });
});

describe('DateTimePicker — Clear button', () => {
  it.each([
    { value: '2026-03-15T12:00:00Z', nullable: true, shouldShow: true },
    { value: '2026-03-15T12:00:00Z', nullable: false, shouldShow: false },
    { value: null, nullable: true, shouldShow: false },
  ])(
    'Clear button visibility with value=$value, nullable=$nullable',
    ({ value, nullable, shouldShow }) => {
      const { queryByRole } = renderPicker({ value, nullable });
      if (shouldShow) {
        expect(queryByRole('button', { name: /clear/i })).toBeTruthy();
      } else {
        expect(queryByRole('button', { name: /clear/i })).toBeNull();
      }
    },
  );

  it('calls onchange(null) when Clear is clicked', async () => {
    const { getByRole, onchange } = renderPicker({ value: '2026-03-15T12:00:00Z', nullable: true });
    await fireEvent.click(getByRole('button', { name: /clear/i }));
    expect(onchange).toHaveBeenCalledWith(null);
  });
});

describe('DateTimePicker — disabled state', () => {
  it.each([
    { disabled: true, expected: true },
    { disabled: undefined, expected: false },
  ])('picker disabled=$expected when disabled=$disabled', ({ disabled, expected }) => {
    const { getByRole } = renderPicker({
      value: null,
      ...(disabled !== undefined ? { disabled } : {}),
    });
    expect(
      (getByRole('button', { name: /choose date and time/i }) as HTMLButtonElement).disabled,
    ).toBe(expected);
  });
});

describe('DateTimePicker — label', () => {
  it('renders label text when provided', () => {
    const { getByText } = renderPicker({ value: null, label: 'Deadline' });
    expect(getByText('Deadline')).toBeTruthy();
  });

  it('renders no label element when label is not provided', () => {
    const { queryByText } = renderPicker({ value: null });
    // No label element — just check the picker renders at all
    const dateInput = queryByText('Date');
    // The input uses aria-label="Date", not a visible text "Date"
    expect(dateInput).toBeNull();
  });
});

describe('DateTimePicker — popover', () => {
  it('opens the calendar and shortcut panels from the trigger', async () => {
    const { container, getByRole, getByText } = renderPicker({ value: null });
    await fireEvent.click(getByRole('button', { name: /choose date and time/i }));

    expect(getByText('Quick dates')).toBeTruthy();
    expect(container.querySelector('button[data-shortcut="today"]')).toBeTruthy();
    expect(container.querySelector('.time-menu button[data-time="09:00"]')).toBeNull();
  });

  it('closes after choosing a calendar day', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(TEST_NOW);

    const { container, getByRole, queryByText } = renderPicker({ value: null });
    await fireEvent.click(getByRole('button', { name: /choose date and time/i }));
    await fireEvent.click(
      container.querySelector('button[data-date="2026-01-01"]') as HTMLButtonElement,
    );

    expect(queryByText('Quick dates')).toBeNull();
  });

  it('keeps only the chosen quick date highlighted when options share a date', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(TEST_NOW);

    const { container, getByRole } = renderPicker({ value: null, defaultTime: '23:59' });

    await fireEvent.click(getByRole('button', { name: /choose date and time/i }));
    await clickShortcut(container, 'tomorrow');
    await fireEvent.click(getByRole('button', { name: /selected tomorrow at 23:59/i }));

    const activeShortcuts = Array.from(
      container.querySelectorAll('.shortcut-btn.option-btn--active'),
    );
    expect(activeShortcuts).toHaveLength(1);
    expect((activeShortcuts[0] as HTMLButtonElement).dataset.shortcut).toBe('tomorrow');
  });
});
