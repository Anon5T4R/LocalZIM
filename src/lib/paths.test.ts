import { describe, expect, it } from "vitest";
import { encodeEntryPath, formatBytes, pathFromHref } from "./paths";

describe("encodeEntryPath", () => {
  it("preserva as barras e escapa o resto", () => {
    expect(encodeEntryPath("A/Café com açúcar?.html")).toBe(
      "A/Caf%C3%A9%20com%20a%C3%A7%C3%BAcar%3F.html"
    );
  });
});

describe("pathFromHref", () => {
  it("extrai id e caminho de URLs do protocolo zim (Windows e Linux)", () => {
    expect(pathFromHref("http://zim.localhost/abc123/C/Foo%20Bar")).toEqual({
      id: "abc123",
      path: "C/Foo Bar",
    });
    expect(pathFromHref("zim://localhost/abc/A/Main.html")).toEqual({
      id: "abc",
      path: "A/Main.html",
    });
  });

  it("devolve null para entradas inválidas", () => {
    expect(pathFromHref("not a url")).toBeNull();
    expect(pathFromHref("http://zim.localhost/so-id")).toBeNull();
  });
});

describe("formatBytes", () => {
  it("formata unidades legíveis", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(97 * 1024 * 1024 * 1024)).toBe("97 GB");
  });
});
