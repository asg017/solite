/*
 * Tiny loadable SQLite extension for exercising `solite docs inline
 * --extension` in tests/test_docs.py.
 *
 * Registers:
 *   - documented_func(a, b): a + b
 *   - undocumented_func(...): 42, registered with two arities so the
 *     undocumented report's overload dedupe is exercised
 */
#include "sqlite3ext.h"
SQLITE_EXTENSION_INIT1

static void documented_func(sqlite3_context *context, int argc,
                            sqlite3_value **argv) {
  (void)argc;
  sqlite3_result_int64(context, sqlite3_value_int64(argv[0]) +
                                    sqlite3_value_int64(argv[1]));
}

static void undocumented_func(sqlite3_context *context, int argc,
                              sqlite3_value **argv) {
  (void)argc;
  (void)argv;
  sqlite3_result_int(context, 42);
}

#ifdef _WIN32
__declspec(dllexport)
#endif
int sqlite3_docsext_init(sqlite3 *db, char **pzErrMsg,
                         const sqlite3_api_routines *pApi) {
  int rc;
  (void)pzErrMsg;
  SQLITE_EXTENSION_INIT2(pApi);
  rc = sqlite3_create_function(db, "documented_func", 2,
                               SQLITE_UTF8 | SQLITE_DETERMINISTIC, 0,
                               documented_func, 0, 0);
  if (rc != SQLITE_OK) return rc;
  rc = sqlite3_create_function(db, "undocumented_func", 0, SQLITE_UTF8, 0,
                               undocumented_func, 0, 0);
  if (rc != SQLITE_OK) return rc;
  rc = sqlite3_create_function(db, "undocumented_func", 1, SQLITE_UTF8, 0,
                               undocumented_func, 0, 0);
  return rc;
}
