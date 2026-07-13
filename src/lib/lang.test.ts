import { describe, expect, it } from "vitest";
import { guessLang } from "./lang";

describe("guessLang", () => {
  it("reconhece códigos ISO de 2 e 3 letras", () => {
    expect(guessLang("en")).toBe("en");
    expect(guessLang("eng")).toBe("en");
    expect(guessLang("pt")).toBe("pt");
    expect(guessLang("por")).toBe("pt");
    expect(guessLang("es")).toBe("es");
    expect(guessLang("spa")).toBe("es");
  });

  it("aceita variantes regionais e listas", () => {
    expect(guessLang("pt-BR")).toBe("pt");
    expect(guessLang("en_US")).toBe("en");
    expect(guessLang("spa;por")).toBe("es");
    expect(guessLang("ENG")).toBe("en");
  });

  it("idioma fora do trio vira null", () => {
    expect(guessLang("fra")).toBe(null);
    expect(guessLang("deu")).toBe(null);
    expect(guessLang("")).toBe(null);
  });
});
