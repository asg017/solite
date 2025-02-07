// SOURCE: https://sqlite.org/forum/forumpost/6c32cec36f4d36ce
/*
** 2020-03-27
**
** The author disclaims copyright to this source code.  In place of
** a legal notice, here is a blessing:
**
**    May you do good and not evil.
**    May you find forgiveness for yourself and forgive others.
**    May you share freely, never taking more than you give.
**
******************************************************************************
**
** Implement a collating sequence that sorts embedded unsigned integers
** in numeric order.
*/
#include "sqlite3ext.h"
SQLITE_EXTENSION_INIT1
#include <ctype.h>
#include <string.h>

/*
** Collating function that compares text byte-by-byte but compares
** digits in numeric order.
*/
static int natSortCollFunc(
  void *notUsed,
  int nKey1, const void *pKey1,
  int nKey2, const void *pKey2
){
  const unsigned char *zA = (const unsigned char*)pKey1;
  const unsigned char *zB = (const unsigned char*)pKey2;
  int i=0, j=0, x;
  while( i<nKey1 && j<nKey2 ){
    x = zA[i] - zB[j];
    if( isdigit(zA[i]) ){
      int k;
      if( !isdigit(zB[j]) ) return x;
      while( zA[i]=='0' && i<nKey1 ){ i++; }
      while( zB[j]=='0' && j<nKey2 ){ j++; }
      k = 0;
      while( i+k<nKey1 && isdigit(zA[i+k]) && j+k<nKey2 && isdigit(zB[j+k]) ){
        k++;
      }
      if( i+k<nKey1 && isdigit(zA[i+k]) ){
        return +1;
      }else if( j+k<nKey2 && isdigit(zB[j+k]) ){
        return -1;
      }else{
        x = memcmp(zA+i, zB+j, k);
        if( x ) return x;
        i += k;
        j += k;
      }
    }else if( x ){
      return x;
    }else{
      i++;
      j++;
    }
  }
  return (nKey1 - i) - (nKey2 - j);
}


#ifdef _WIN32
__declspec(dllexport)
#endif
int sqlite3_natsort_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  int rc = SQLITE_OK;
  SQLITE_EXTENSION_INIT2(pApi);
  (void)pzErrMsg;  /* Unused parameter */
  rc = sqlite3_create_collation(db, "natsort", SQLITE_UTF8, 0, natSortCollFunc);
  return rc;
}
