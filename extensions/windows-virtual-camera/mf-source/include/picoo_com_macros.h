#pragma once

#include <winerror.h>

#ifndef RETURN_IF_FAILED
#define RETURN_IF_FAILED(expr)             \
    do {                                   \
        const HRESULT _hr = (expr);        \
        if (FAILED(_hr)) {                 \
            return _hr;                    \
        }                                  \
    } while (0)
#endif

#ifndef RETURN_HR_IF
#define RETURN_HR_IF(hr, condition) \
    do {                            \
        if (condition) {            \
            return (hr);            \
        }                           \
    } while (0)
#endif

#ifndef RETURN_HR_IF_NULL
#define RETURN_HR_IF_NULL(hr, pointer) RETURN_HR_IF((hr), (pointer) == nullptr)
#endif
