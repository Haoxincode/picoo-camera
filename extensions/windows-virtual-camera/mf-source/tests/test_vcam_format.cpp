// Linux-hostable unit test for picoo_vcam_format.h — REQ-PICOO-VCAM-002.
#include "picoo_vcam_format.h"

#include <cstdio>

static int failures = 0;

static void expect_eq(uint32_t got, uint32_t want, const char* label) {
    if (got != want) {
        std::fprintf(stderr, "FAIL %s: got %u want %u\n", label, got, want);
        ++failures;
    }
}

static void expect_true(int cond, const char* label) {
    if (!cond) {
        std::fprintf(stderr, "FAIL %s\n", label);
        ++failures;
    }
}

int main() {
    uint32_t w = 0;
    uint32_t h = 0;

    picoo_select_output_nv12_dims(1280, 720, &w, &h);
    expect_eq(w, 1280, "720p width");
    expect_eq(h, 720, "720p height");

    picoo_select_output_nv12_dims(1920, 1080, &w, &h);
    expect_eq(w, 1920, "1080p width");
    expect_eq(h, 1080, "1080p height");

    picoo_select_output_nv12_dims(640, 480, &w, &h);
    expect_eq(w, 1280, "sub-720 maps to 720p width");
    expect_eq(h, 720, "sub-720 maps to 720p height");

    picoo_select_output_nv12_dims(3840, 2160, &w, &h);
    expect_eq(w, 1920, "4k maps to 1080p width");
    expect_eq(h, 1080, "4k maps to 1080p height");

    expect_true(picoo_output_dims_need_update(1280, 720, 1920, 1080),
                "720→1080 needs update");
    expect_true(!picoo_output_dims_need_update(1920, 1080, 1920, 1080),
                "1080→1080 no update");
    expect_true(picoo_output_dims_need_update(1920, 1080, 1280, 720),
                "1080→720 needs update");

    if (failures != 0) {
        std::fprintf(stderr, "%d assertion(s) failed\n", failures);
        return 1;
    }
    std::puts("ok picoo_vcam_format");
    return 0;
}
