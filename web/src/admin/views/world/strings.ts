// The text of the world category outside the map itself (the Instances & layers tab and the layer popover). The map's own text is `world/strings.ts`.
export const W = {
  instances: {
    title: 'Instances and layers',
    empty: 'No live layer right now.',
    world: 'World',
    layer: 'Layer',
    kind: 'Kind',
    owner: 'Owner',
    members: 'Members',
    monsters: 'Monsters alive',
    age: 'Age',
    left: 'Time left',
    open: 'Open on map',
    refused: 'The server did not send the layer list (it needs view.map).',
    waiting: 'Waiting for the layer list...',
    kinds: {
      normal: 'normal',
      ticket: 'ticket',
      instance: 'instance',
      guild_house: 'guild house',
      mini_room: 'mini-room',
    } as Readonly<Record<string, string>>,
  },
  popover: {
    title: 'Live instances',
    none: 'No live instance: showing the empty template layer.',
    members: (n: number): string => `${String(n)} in`,
    open: 'Open the template layer',
  },
} as const;
