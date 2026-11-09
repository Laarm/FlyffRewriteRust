// The texts of the World & Content tabs (Plan/25 section 2.3: all strings in one strings.ts).

const n = (v: number): string => v.toLocaleString('en-US');
const plural = (c: number, one: string, many = `${one}s`): string =>
  `${n(c)} ${c === 1 ? one : many}`;

export const T = {
  rates: {
    title: 'Rates',
    hint: 'A change is live at once for everybody and is kept over a restart. The effective value is override, else flyff.toml, else the value of the data.',
    rate: 'Rate',
    dataDefault: 'Data',
    toml: 'flyff.toml',
    override: 'Override',
    effective: 'Effective',
    setTo: 'Set to',
    apply: 'Apply',
    reset: 'Reset',
    resetHint: 'Back to the value of flyff.toml or the data',
    sliderLabel: (label: string): string => `${label} slider`,
    inputLabel: (label: string): string => `${label} value`,
    range: (lo: string, hi: string, exclusive: boolean): string =>
      exclusive ? `more than ${lo}, up to ${hi}` : `${lo} to ${hi}`,
    layers: { override: 'override', toml: 'flyff.toml', default: 'data' },
    confirmTitle: (label: string, v: string): string => `Set ${label} to ${v}?`,
    confirmBody:
      'It takes effect for everybody at once. Players see the new rate in their game settings.',
    resetTitle: (label: string): string => `Reset ${label}?`,
    resetBody:
      'The override is removed: the value of flyff.toml (or of the data) is in force again.',
    applied: (label: string, v: string): string => `${label} is now ${v}.`,
    wasReset: (label: string, v: string): string => `${label} is back to ${v}.`,
    outOfRange: 'Outside the allowed range.',
  },
  events: {
    title: 'Event flags',
    hint: 'The switches the game reads (PK, respawn, guild systems, ...). A change is live at once and kept over a restart. Flag 1 cannot be changed at runtime; generic events (500 and up) are set by their timer.',
    id: 'Id',
    name: 'Flag',
    description: 'What it does',
    state: 'State',
    on: 'On',
    off: 'Off',
    turnOn: 'Turn on',
    turnOff: 'Turn off',
    overridden: 'override',
    protectedFlag: 'protected',
    generic: 'generic',
    set: 'Set',
    value: 'Value',
    valueLabel: (id: number): string => `State of flag ${String(id)}`,
    setOther: 'Set another flag',
    otherHint: 'An unnamed flag, 0 to 499, to a state of 0 to 255.',
    flagId: 'Flag id',
    changed: (name: string, state: number): string => `${name} is now ${String(state)}.`,
    confirmTitle: (name: string, state: number): string => `Set ${name} to ${String(state)}?`,
    confirmBody: 'The flag changes at once for the whole server and every player is told.',
    badId: 'A flag id is 0 to 499.',
    badState: 'A state is 0 to 255.',
    empty: 'No flags.',
  },
  modifiers: {
    title: 'Event.lua modifiers',
    hint: 'The factors the running events of Event.lua give (read only). Change them by editing Event.lua and reloading it.',
    label: 'Modifier',
    value: 'Value',
    source: 'Source',
    active: 'active',
    running: 'Running events',
    noneRunning: 'No Lua event is running.',
    format: (v: number, unit: string): string => {
      if (unit === '+') return v > 0 ? `+${n(v)}` : n(v);
      if (unit === 'ms') return `${n(v)} ms`;
      return `x${n(Math.round(v * 1000) / 1000)}`;
    },
  },
  weather: {
    title: 'Weather',
    state: (effect: boolean, title: string): string =>
      effect
        ? `A weather effect is on${title === '' ? '' : `: ${title}`}.`
        : 'The weather is clear.',
    world: 'World',
    pick: 'Pick a world',
    clear: 'Clear',
    rain: 'Rain',
    snow: 'Snow',
    needs: 'Changing the weather needs',
    noteGlobal:
      'The weather effect is one switch for the whole server in this build; rain falls for every world at once.',
    done: (world: string, kind: string): string => `${kind} in ${world}.`,
  },
  spawns: {
    world: 'World',
    pick: 'Pick a world',
    find: 'Find a monster',
    onlyMissing: 'Only lines below their target',
    onlyMonsters: 'Monsters only',
    line: 'Line',
    what: 'Spawns',
    area: 'Area',
    target: 'Target',
    alive: 'Alive',
    awake: 'Awake',
    aggressive: 'Aggressive',
    next: 'Next respawn',
    actions: '',
    respawnNow: 'Respawn now',
    respawnTitle: (name: string): string => `Respawn ${name} now?`,
    respawnBody:
      'The line is refilled to its target at the next respawn pass, within a second or two.',
    respawned: (name: string, missing: number): string =>
      missing === 0
        ? `${name} is already full.`
        : `${name}: ${plural(missing, 'monster')} on the way.`,
    openMap: 'Open on the map',
    mapShort: 'Map',
    full: 'full',
    due: 'due now',
    paused: 'paused (day/hour window)',
    off: 'Respawning is switched off (events.respawn = 0): nothing refills.',
    gameTime: (d: number, h: number): string => `In-game day ${String(d)}, hour ${String(h)}.`,
    lines: (c: number): string => plural(c, 'line'),
    fixed: 'fixed spot',
    flying: 'flyer',
    empty: 'This world has no respawn lines.',
    emptyHint: 'Instances, dungeons and indoor worlds usually have none.',
    countdown: (s: number): string =>
      s >= 3600
        ? `${(s / 3600).toFixed(1)} h`
        : s >= 90
          ? `${Math.round(s / 60)} min`
          : `${String(s)} s`,
    layerNote: 'The counters are those of the main layer (layer 0).',
  },
  census: {
    title: 'Monster census',
    hint: 'Every living monster of the world by rank, asleep (nobody sees them, so they do not run) or awake. Pets are not counted.',
    alive: 'Alive',
    awake: 'Awake',
    asleep: 'Asleep',
    players: 'Players',
    npcs: 'NPCs',
    layers: 'Layers',
    rank: 'Rank',
    ranks: ['Normal', 'Captain', 'Mid-boss', 'Boss', 'Giant', 'Guard'],
    top: 'Most numerous types',
    type: 'Monster',
    level: 'Level',
  },
  content: {
    title: 'Content panels',
    hint: 'One panel per system of the later phases. A panel shows its data and controls once its system exists.',
    reads: 'It will show',
    actions: 'It will offer',
    readOnly: 'Read only.',
    select: 'Panels',
    soon: 'Not available yet',
  },
} as const;
