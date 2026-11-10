// Launch eligibility is supplied by core. Missing/older IPC data fails closed.
export type FilePresentation = { name: string | null; can_open?: boolean };
export const canOpen = (file: FilePresentation) => file.can_open === true;
export const fileName = (file: FilePresentation, fallback: string) =>
  file.name?.replace(/[\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "") || fallback;
