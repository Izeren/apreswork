// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import type { Priority, TaskStatus } from '../../types';
import type { SortKey, SortDirection } from './taskSort';
import {
  DEFAULT_SORT_STACK,
  clickSortField,
  sortTasks,
  compareByPriority,
  compareByDeadline,
  compareByTitle,
  compareByStatus,
  compareByLogged,
} from './taskSort';
import { baseTask } from './testFixtures';

describe('sortTasks — by priority descending', () => {
  const priorityCases: Array<{ priorities: Priority[]; expectedOrder: Priority[] }> = [
    {
      priorities: ['Low', 'Medium', 'High', 'Critical'],
      expectedOrder: ['Critical', 'High', 'Medium', 'Low'],
    },
    {
      priorities: ['Critical', 'High', 'Medium', 'Low'],
      expectedOrder: ['Critical', 'High', 'Medium', 'Low'],
    },
    {
      priorities: ['Medium', 'Low', 'Critical', 'High'],
      expectedOrder: ['Critical', 'High', 'Medium', 'Low'],
    },
    {
      priorities: ['High', 'High', 'Low'],
      expectedOrder: ['High', 'High', 'Low'],
    },
  ];

  it.each(priorityCases)('sorts $priorities → $expectedOrder', ({ priorities, expectedOrder }) => {
    const tasks = priorities.map((priority, i) => baseTask({ id: `t${i}`, priority }));
    const sorted = sortTasks(tasks, [{ field: 'priority', direction: 'desc' }]);
    expect(sorted.map((t) => t.priority)).toEqual(expectedOrder);
  });

  it('sort by priority ascending (Low first)', () => {
    const tasks = [
      baseTask({ id: 't1', priority: 'Critical' }),
      baseTask({ id: 't2', priority: 'Low' }),
      baseTask({ id: 't3', priority: 'High' }),
    ];
    const sorted = sortTasks(tasks, [{ field: 'priority', direction: 'asc' }]);
    expect(sorted.map((t) => t.priority)).toEqual(['Low', 'High', 'Critical']);
  });

  it('returns empty array when given empty input', () => {
    expect(sortTasks([], [{ field: 'priority', direction: 'desc' }])).toEqual([]);
  });

  it('does not mutate the original array', () => {
    const tasks = [
      baseTask({ id: 't1', priority: 'Low' }),
      baseTask({ id: 't2', priority: 'Critical' }),
    ];
    const original = [...tasks];
    sortTasks(tasks, [{ field: 'priority', direction: 'desc' }]);
    expect(tasks).toEqual(original);
  });
});

describe('sortTasks — by deadline', () => {
  const deadlineCases: Array<{
    deadlines: (string | null)[];
    expectedDeadlines: (string | null)[];
    direction: SortDirection;
  }> = [
    {
      deadlines: ['2026-12-01T00:00:00Z', '2026-03-01T00:00:00Z', '2026-06-01T00:00:00Z'],
      expectedDeadlines: ['2026-03-01T00:00:00Z', '2026-06-01T00:00:00Z', '2026-12-01T00:00:00Z'],
      direction: 'asc',
    },
    {
      deadlines: [null, '2026-03-01T00:00:00Z', null],
      expectedDeadlines: ['2026-03-01T00:00:00Z', null, null],
      direction: 'asc',
    },
    {
      deadlines: [null, null],
      expectedDeadlines: [null, null],
      direction: 'asc',
    },
    {
      deadlines: ['2026-01-01T00:00:00Z'],
      expectedDeadlines: ['2026-01-01T00:00:00Z'],
      direction: 'asc',
    },
    {
      deadlines: ['2026-03-01T00:00:00Z', '2026-12-01T00:00:00Z', null],
      expectedDeadlines: ['2026-12-01T00:00:00Z', '2026-03-01T00:00:00Z', null],
      direction: 'desc',
    },
  ];

  it.each(deadlineCases)(
    'sorts $deadlines with direction $direction → $expectedDeadlines',
    ({ deadlines, expectedDeadlines, direction }) => {
      const tasks = deadlines.map((deadline, i) => baseTask({ id: `t${i}`, deadline }));
      const sorted = sortTasks(tasks, [{ field: 'deadline', direction }]);
      expect(sorted.map((t) => t.deadline)).toEqual(expectedDeadlines);
    },
  );

  it('all null deadlines remain in original relative order', () => {
    const tasks = [baseTask({ id: 'a', deadline: null }), baseTask({ id: 'b', deadline: null })];
    const sorted = sortTasks(tasks, [{ field: 'deadline', direction: 'asc' }]);
    expect(sorted.map((t) => t.id)).toEqual(['a', 'b']);
  });
});

describe('sortTasks — by title', () => {
  const titleCases: Array<{
    titles: string[];
    expectedTitles: string[];
    direction: SortDirection;
  }> = [
    {
      titles: ['Zebra', 'Alpha', 'Middle'],
      expectedTitles: ['Alpha', 'Middle', 'Zebra'],
      direction: 'asc',
    },
    {
      titles: ['alpha', 'Beta', 'alpha'],
      expectedTitles: ['alpha', 'alpha', 'Beta'],
      direction: 'asc',
    },
    { titles: ['a'], expectedTitles: ['a'], direction: 'asc' },
    { titles: ['Alpha', ''], expectedTitles: ['', 'Alpha'], direction: 'asc' },
    {
      titles: ['Apple', 'Zebra', 'Mango'],
      expectedTitles: ['Zebra', 'Mango', 'Apple'],
      direction: 'desc',
    },
  ];

  it.each(titleCases)(
    'sorts $titles with direction $direction → $expectedTitles',
    ({ titles, expectedTitles, direction }) => {
      const tasks = titles.map((title, i) => baseTask({ id: `t${i}`, title }));
      const sorted = sortTasks(tasks, [{ field: 'title', direction }]);
      expect(sorted.map((t) => t.title)).toEqual(expectedTitles);
    },
  );
});

describe('sortTasks — by status', () => {
  const statusCases: Array<{
    statuses: TaskStatus[];
    expectedOrder: TaskStatus[];
    direction: SortDirection;
  }> = [
    {
      statuses: ['cancelled', 'completed', 'scheduled', 'pending', 'backlog'],
      expectedOrder: ['backlog', 'pending', 'scheduled', 'completed', 'cancelled'],
      direction: 'asc',
    },
    {
      statuses: ['completed', 'backlog', 'scheduled'],
      expectedOrder: ['backlog', 'scheduled', 'completed'],
      direction: 'asc',
    },
    {
      statuses: ['pending', 'pending'],
      expectedOrder: ['pending', 'pending'],
      direction: 'asc',
    },
    {
      statuses: ['backlog', 'cancelled', 'scheduled'],
      expectedOrder: ['cancelled', 'scheduled', 'backlog'],
      direction: 'desc',
    },
  ];

  it.each(statusCases)(
    'sorts $statuses with direction $direction → $expectedOrder',
    ({ statuses, expectedOrder, direction }) => {
      const tasks = statuses.map((status, i) => baseTask({ id: `t${i}`, status }));
      const sorted = sortTasks(tasks, [{ field: 'status', direction }]);
      expect(sorted.map((t) => t.status)).toEqual(expectedOrder);
    },
  );
});

describe('sortTasks — by logged minutes', () => {
  it.each([
    {
      label: 'ascending (least logged first)',
      direction: 'asc' as const,
      logged: [90, 0, 30],
      expected: [0, 30, 90],
    },
    {
      label: 'descending (most logged first)',
      direction: 'desc' as const,
      logged: [30, 90, 0],
      expected: [90, 30, 0],
    },
  ])('$label', ({ direction, logged, expected }) => {
    const tasks = logged.map((m, i) => baseTask({ id: `t${i + 1}`, time_logged_minutes: m }));
    const sorted = sortTasks(tasks, [{ field: 'logged', direction }]);
    expect(sorted.map((t) => t.time_logged_minutes)).toEqual(expected);
  });
});

describe('sortTasks — multi-key stack', () => {
  it('breaks primary-key ties with the second key (default stack)', () => {
    const tasks = [
      baseTask({ id: 'high-late', priority: 'High', deadline: '2026-12-01T00:00:00Z' }),
      baseTask({ id: 'low', priority: 'Low', deadline: '2026-01-01T00:00:00Z' }),
      baseTask({ id: 'high-early', priority: 'High', deadline: '2026-03-01T00:00:00Z' }),
      baseTask({ id: 'high-none', priority: 'High', deadline: null }),
    ];
    const sorted = sortTasks(tasks, DEFAULT_SORT_STACK);
    // Priority groups first (High before Low); within High, earlier deadline
    // first and null last.
    expect(sorted.map((t) => t.id)).toEqual(['high-early', 'high-late', 'high-none', 'low']);
  });

  it('applies a third key when the first two tie', () => {
    const stack: SortKey[] = [
      { field: 'priority', direction: 'desc' },
      { field: 'status', direction: 'asc' },
      { field: 'title', direction: 'asc' },
    ];
    const tasks = [
      baseTask({ id: 'b', priority: 'High', status: 'scheduled', title: 'Bravo' }),
      baseTask({ id: 'a', priority: 'High', status: 'scheduled', title: 'Alpha' }),
      baseTask({ id: 'backlog', priority: 'High', status: 'backlog', title: 'Zulu' }),
    ];
    const sorted = sortTasks(tasks, stack);
    expect(sorted.map((t) => t.id)).toEqual(['backlog', 'a', 'b']);
  });

  it('keeps input order on a full tie (stable sort)', () => {
    const tasks = [
      baseTask({ id: 'first', priority: 'Medium', deadline: null }),
      baseTask({ id: 'second', priority: 'Medium', deadline: null }),
      baseTask({ id: 'third', priority: 'Medium', deadline: null }),
    ];
    const sorted = sortTasks(tasks, DEFAULT_SORT_STACK);
    expect(sorted.map((t) => t.id)).toEqual(['first', 'second', 'third']);
  });

  it('an empty stack keeps input order', () => {
    const tasks = [
      baseTask({ id: 'z', priority: 'Low', title: 'Zebra' }),
      baseTask({ id: 'a', priority: 'Critical', title: 'Alpha' }),
    ];
    const sorted = sortTasks(tasks, []);
    expect(sorted.map((t) => t.id)).toEqual(['z', 'a']);
  });
});

describe('clickSortField', () => {
  it('toggles the primary key direction, keeping the rest of the stack', () => {
    const once = clickSortField(DEFAULT_SORT_STACK, 'priority');
    expect(once).toEqual([
      { field: 'priority', direction: 'asc' },
      { field: 'deadline', direction: 'asc' },
    ]);
    const twice = clickSortField(once, 'priority');
    expect(twice).toEqual([...DEFAULT_SORT_STACK]);
  });

  it.each([
    { field: 'priority', direction: 'desc' },
    { field: 'status', direction: 'asc' },
    { field: 'deadline', direction: 'asc' },
    { field: 'title', direction: 'asc' },
    { field: 'logged', direction: 'asc' },
  ] as const)(
    'promotes $field to primary with default direction $direction',
    ({ field, direction }) => {
      // Start from a stack where the clicked field is never primary.
      const stack: SortKey[] =
        field === 'title'
          ? [{ field: 'logged', direction: 'desc' }]
          : [{ field: 'title', direction: 'desc' }];
      const next = clickSortField(stack, field);
      expect(next[0]).toEqual({ field, direction });
      expect(next.slice(1)).toEqual(stack);
    },
  );

  it('removes the promoted field’s previous entry (no duplicate fields)', () => {
    // Default stack already contains a deadline entry — promoting deadline
    // must move it to the front, not duplicate it.
    const next = clickSortField(DEFAULT_SORT_STACK, 'deadline');
    expect(next).toEqual([
      { field: 'deadline', direction: 'asc' },
      { field: 'priority', direction: 'desc' },
    ]);
  });

  it('discards a demoted key’s toggled direction on re-promotion', () => {
    // deadline toggled to desc as primary, then priority promoted over it,
    // then deadline clicked again: it returns with its DEFAULT direction.
    let stack = clickSortField(DEFAULT_SORT_STACK, 'deadline'); // deadline asc primary
    stack = clickSortField(stack, 'deadline'); // deadline desc
    stack = clickSortField(stack, 'priority'); // priority desc primary
    stack = clickSortField(stack, 'deadline');
    expect(stack).toEqual([
      { field: 'deadline', direction: 'asc' },
      { field: 'priority', direction: 'desc' },
    ]);
  });

  it('grows the stack as new fields are clicked, capped at the five fields', () => {
    let stack: readonly SortKey[] = DEFAULT_SORT_STACK;
    for (const field of ['status', 'title', 'logged', 'deadline', 'priority'] as const) {
      stack = clickSortField(stack, field);
    }
    expect(stack.map((k) => k.field)).toEqual([
      'priority',
      'deadline',
      'logged',
      'title',
      'status',
    ]);
    expect(stack).toHaveLength(5);
  });
});

describe('compareByStatus', () => {
  it('equal statuses return 0', () => {
    const a = baseTask({ status: 'scheduled' });
    const b = baseTask({ status: 'scheduled' });
    // Use toEqual rather than toBe to treat -0 and +0 as equal
    expect(compareByStatus(a, b, 'desc')).toEqual(0);
  });

  it('backlog sorts before completed for asc sort', () => {
    const a = baseTask({ status: 'backlog' });
    const b = baseTask({ status: 'completed' });
    expect(compareByStatus(a, b, 'asc')).toBeLessThan(0);
  });
});

describe('compareByLogged', () => {
  it('equal logged minutes return 0', () => {
    const a = baseTask({ time_logged_minutes: 45 });
    const b = baseTask({ time_logged_minutes: 45 });
    // Use toEqual rather than toBe to treat -0 and +0 as equal
    expect(compareByLogged(a, b, 'desc')).toEqual(0);
  });
});

describe('compareByPriority', () => {
  it('Critical < High for desc sort (negative means Critical comes first)', () => {
    const a = baseTask({ priority: 'Critical' });
    const b = baseTask({ priority: 'High' });
    expect(compareByPriority(a, b, 'desc')).toBeLessThan(0);
  });

  it('equal priorities return 0', () => {
    const a = baseTask({ priority: 'Medium' });
    const b = baseTask({ priority: 'Medium' });
    // Use toEqual rather than toBe to treat -0 and +0 as equal
    expect(compareByPriority(a, b, 'desc')).toEqual(0);
  });
});

describe('compareByDeadline', () => {
  it.each([
    { a: null, b: '2026-01-01T00:00:00Z', direction: 'asc' as const, expectedSign: 1 },
    { a: null, b: '2026-01-01T00:00:00Z', direction: 'desc' as const, expectedSign: 1 },
    { a: null, b: null, direction: 'asc' as const, expectedSign: 0 },
  ])(
    'null handling (a=$a, b=$b, dir=$direction) → sign $expectedSign',
    ({ a, b, direction, expectedSign }) => {
      const result = compareByDeadline(
        baseTask({ deadline: a }),
        baseTask({ deadline: b }),
        direction,
      );
      expect(Math.sign(result)).toEqual(expectedSign);
    },
  );
});

describe('compareByTitle', () => {
  it('identical titles return 0', () => {
    const a = baseTask({ title: 'Same' });
    const b = baseTask({ title: 'Same' });
    expect(compareByTitle(a, b)).toBe(0);
  });
});
