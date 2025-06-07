// SOURCE: https://sqlite.org/forum/forumpost/6c32cec36f4d36ce
#include "sqlite3ext.h"
SQLITE_EXTENSION_INIT1

static void shellUSleepFunc(
  sqlite3_context *context,
  int argcUnused,
  sqlite3_value **argv
){
  int sleep = sqlite3_value_int(argv[0]);
  (void)argcUnused;
  sqlite3_sleep(sleep);
  sqlite3_result_int(context, sleep);
}

#ifdef _WIN32
__declspec(dllexport)
#endif
int sqlite3_usleep_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  int rc = SQLITE_OK;
  SQLITE_EXTENSION_INIT2(pApi);
  (void)pzErrMsg;  /* Unused parameter */
  return sqlite3_create_function(db, "usleep",1,SQLITE_UTF8,0,
                            shellUSleepFunc, 0, 0);
}
