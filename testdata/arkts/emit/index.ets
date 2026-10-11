export interface Entry {
  name: string;
}

export function label(entry: Entry): string {
  return `Hello ${entry.name}`;
}
