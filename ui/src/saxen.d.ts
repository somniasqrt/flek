declare module "saxen" {
  type Decode = (value: string) => string;
  export class Parser {
    on(
      event: "openTag",
      handler: (
        tag: string,
        attrs: () => Record<string, string>,
        decode: Decode,
        selfClosing: boolean,
      ) => void,
    ): void;
    on(
      event: "closeTag",
      handler: (tag: string, decode: Decode, selfClosing: boolean) => void,
    ): void;
    on(event: "text", handler: (text: string, decode: Decode) => void): void;
    on(event: "cdata", handler: (text: string) => void): void;
    on(event: "error" | "warn", handler: () => void): void;
    parse(xml: string): void;
  }
}
