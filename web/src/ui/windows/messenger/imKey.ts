// The key of an IM window: the lower-cased partner name (`instanceKey` of `APP_MESSAGE`, Plan/42 §19.4 I). The original keyed its `CMapStringToPtr` by the exact name;
// the character names of this server are unique case-insensitively, so one window per partner is one window per key.
export const imKey = (name: string): string => name.toLowerCase();
