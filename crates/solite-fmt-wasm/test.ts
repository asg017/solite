import { assertEquals, assertStringIncludes } from "https://deno.land/std@0.220.0/assert/mod.ts";

// Import the WASM module (after wasm-pack build --target web)
import init, { format, check } from "./pkg/solite_fmt_wasm.js";

Deno.test("solite_fmt_wasm", async (t) => {
  // Initialize WASM module
  await init();

  await t.step("format with defaults", () => {
    const result = format("SELECT a,b,c FROM t WHERE x=1", undefined);
    assertStringIncludes(result, "select");
    assertStringIncludes(result, "from");
    assertStringIncludes(result, "where");
  });

  await t.step("format with uppercase keywords", () => {
    const result = format("select a from t", { keyword_case: "upper" });
    assertStringIncludes(result, "SELECT");
    assertStringIncludes(result, "FROM");
  });

  await t.step("format with custom indent", () => {
    const result = format("SELECT a,b FROM t", { indent_size: 4 });
    // Multi-column select should have indentation
    assertStringIncludes(result, "    ");
  });

  await t.step("format with tabs", () => {
    const result = format("SELECT a,b FROM t", { indent_style: "tabs" });
    assertStringIncludes(result, "\t");
  });

  await t.step("check formatted returns true", () => {
    const result = check("select * from t;\n", undefined);
    assertEquals(result, true);
  });

  await t.step("check unformatted returns false", () => {
    const result = check("SELECT    *    FROM    t", undefined);
    assertEquals(result, false);
  });

  await t.step("format handles parse errors", () => {
    try {
      format("SELECT FROM", undefined);
      throw new Error("Should have thrown");
    } catch (e) {
      assertStringIncludes((e as Error).message, "error");
    }
  });

  await t.step("format multiple statements", () => {
    const result = format("SELECT a FROM t; SELECT b FROM u", undefined);
    // Should format both statements
    assertStringIncludes(result, "select a from t;");
    assertStringIncludes(result, "select b from u;");
  });

  await t.step("format preserves leading comments", () => {
    const result = format("-- this is a comment\nSELECT * FROM t", undefined);
    assertStringIncludes(result, "-- this is a comment");
    assertStringIncludes(result, "select * from t;");
  });

  await t.step("format preserves block comments", () => {
    const result = format("/* block comment */\nSELECT * FROM t", undefined);
    assertStringIncludes(result, "/* block comment */");
  });

  await t.step("format preserves comments between statements", () => {
    const result = format("SELECT a FROM t;\n-- middle comment\nSELECT b FROM u", undefined);
    assertStringIncludes(result, "-- middle comment");
  });
});
