// SPDX-License-Identifier: MPL-2.0
// Linux research executable, AudioFlux parameters remain unchanged
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <float.h>
#include <inttypes.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include "stft_algorithm.h"
#include "mir/onset_algorithm.h"

enum { RATE = 48000, WINDOW = 1024, HOP = 128, BINS = WINDOW / 2 + 1 };
#define MAX_FRAMES (64U * RATE)
#define MEMORY_BUDGET (512ULL * 1024 * 1024)

static void fail(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(2);
}

static size_t number(const char *text, size_t limit) {
    if (!*text) fail("empty integer");
    for (const char *p = text; *p; ++p)
        if (*p < '0' || *p > '9') fail("invalid integer");
    errno = 0;
    uintmax_t value = strtoumax(text, NULL, 10);
    if (errno || value > limit) fail("integer outside supported bounds");
    return (size_t)value;
}

static void *allocate(size_t count, size_t size) {
    if (!count || count > SIZE_MAX / size || count * size > MEMORY_BUDGET)
        fail("allocation outside supported bounds");
    void *value = calloc(count, size);
    if (!value) fail("allocation failed");
    return value;
}

static float *read_pcm(const char *path, size_t channels, size_t channel, size_t frames) {
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    struct stat info;
    uint64_t expected = (uint64_t)frames * channels * 4;
    if (fd < 0 || fstat(fd, &info) || !S_ISREG(info.st_mode)
        || info.st_size < 0 || (uint64_t)info.st_size != expected) {
        if (fd >= 0) close(fd);
        fail("PCM must be a regular F32LE file of the exact declared length");
    }
    FILE *input = fdopen(fd, "rb");
    if (!input) fail("cannot open PCM stream");
    float *samples = allocate(frames, sizeof(float));
    unsigned char bytes[4096 * 8];
    size_t offset = 0;
    while (offset < frames) {
        size_t count = frames - offset < 4096 ? frames - offset : 4096;
        size_t size = count * channels * 4;
        if (fread(bytes, 1, size, input) != size) {
            free(samples);
            fclose(input);
            fail("truncated PCM read");
        }
        for (size_t i = 0; i < count * channels; ++i) {
            const unsigned char *b = bytes + i * 4;
            uint32_t bits = (uint32_t)b[0] | ((uint32_t)b[1] << 8)
                | ((uint32_t)b[2] << 16) | ((uint32_t)b[3] << 24);
            float value;
            memcpy(&value, &bits, sizeof(value));
            if (!isfinite(value) || fabsf(value) > 4.0f) {
                free(samples);
                fclose(input);
                fail("PCM contains a non-finite or out-of-range sample");
            }
            if (i % channels == channel) samples[offset + i / channels] = value;
        }
        offset += count;
    }
    if (fgetc(input) != EOF || ferror(input)) {
        free(samples);
        fclose(input);
        fail("PCM changed length or read failed");
    }
    if (fclose(input)) {
        free(samples);
        fail("PCM close failed");
    }
    return samples;
}

int main(int argc, char **argv) {
    if (argc != 6) fail("usage: native-onset-check PCM CHANNELS CHANNEL FRAMES NEW_OUTPUT.json");
    if (sizeof(float) != 4 || FLT_RADIX != 2 || FLT_MANT_DIG != 24 || FLT_MAX_EXP != 128)
        fail("IEEE binary32 is required");
    size_t channels = number(argv[2], 2), channel = number(argv[3], 1);
    size_t frames = number(argv[4], MAX_FRAMES);
    if (!channels || channel >= channels || !frames) fail("invalid PCM shape");
    float *pcm = read_pcm(argv[1], channels, channel, frames);
    if (frames < WINDOW) {
        free(pcm);
        fail("UNSUPPORTED: fewer than 1024 frames, no padding or invented events");
    }
    size_t rows = (frames - WINDOW) / HOP + 1;
    size_t full = rows * WINDOW, half = rows * BINS;
    // Conservative simultaneous allocation bound, including native scratch and PCM copies
    uint64_t budget = sizeof(float) * (2ULL * full + 4ULL * half + 3ULL * frames)
        + rows * (sizeof(int) + sizeof(float)) + 1024 * 1024;
    if (budget > MEMORY_BUDGET || full > INT32_MAX || half > INT32_MAX) {
        free(pcm);
        fail("UNSUPPORTED: analysis exceeds the 512 MiB working allocation budget");
    }

    STFTObj stft = NULL;
    WindowType window = Window_Hann;
    int hop = HOP, streaming = 0;
    if (stftObj_new(&stft, 10, &window, &hop, &streaming) || !stft)
        fail("native STFT initialization failed");
    if (stftObj_calTimeLength(stft, (int)frames) != (int)rows)
        fail("native STFT shape does not match unpadded coordinates");
    float *real = allocate(full, sizeof(float)), *imaginary = allocate(full, sizeof(float));
    float *magnitude = allocate(half, sizeof(float)), *phase = allocate(half, sizeof(float));
    stftObj_stft(stft, pcm, (int)frames, real, imaginary);
    for (size_t i = 0; i < rows; ++i) {
        for (size_t bin = 0; bin < BINS; ++bin) {
            float re = real[i * WINDOW + bin], im = imaginary[i * WINDOW + bin];
            float mag = hypotf(re, im), angle = atan2f(im, re);
            if (!isfinite(mag) || !isfinite(angle)) fail("native STFT returned non-finite data");
            magnitude[i * BINS + bin] = mag;
            phase[i * BINS + bin] = angle;
        }
    }
    free(real);
    free(imaginary);
    free(pcm);
    stftObj_free(stft);

    OnsetObj onset = NULL;
    NoveltyType type = Novelty_RCD;
    int rate = RATE, order = 1;
    if (onsetObj_new(&onset, (int)rows, BINS, HOP, &rate, &order, &type) || !onset)
        fail("native onset initialization failed");
    float *novelty = allocate(rows, sizeof(float));
    int *points = allocate(rows, sizeof(int));
    int count = onsetObj_onset(onset, magnitude, phase, NULL, NULL, 0, novelty, points);
    if (count < 0 || (size_t)count > rows) fail("native onset returned an invalid count");
    for (size_t i = 0; i < rows; ++i)
        if (!isfinite(novelty[i]) || novelty[i] < 0 || novelty[i] > 1)
            fail("native onset returned invalid normalized novelty");
    for (int i = 0; i < count; ++i)
        if (points[i] < 0 || (size_t)points[i] >= rows
            || (size_t)points[i] * HOP >= frames || (i && points[i] <= points[i - 1]))
            fail("native onset returned unordered, repeated or out-of-range points");

    onsetObj_free(onset);
    free(magnitude);
    free(phase);
    int output_fd = open(argv[5], O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0600);
    if (output_fd < 0) {
        free(novelty);
        free(points);
        fail("cannot create new output, existing paths are never replaced");
    }
    FILE *output = fdopen(output_fd, "w");
    if (!output) fail("cannot open output stream");
    fprintf(output, "{\"algorithm\":\"audioflux-rcd-default-peaks\",\"sample_rate\":%d,"
        "\"sample_frames\":%zu,\"channels\":%zu,\"channel\":%zu,\"window\":\"Hann\","
        "\"window_frames\":%d,\"hop_frames\":%d,\"padding\":false,"
        "\"coordinate\":\"stft_index_times_hop\",\"confidence\":null,"
        "\"working_allocation_bound_bytes\":%" PRIu64 ",\"predicted_frames\":[",
        RATE, frames, channels, channel, WINDOW, HOP, budget);
    for (int i = 0; i < count; ++i)
        fprintf(output, "%s%zu", i ? "," : "", (size_t)points[i] * HOP);
    fputs("],\"novelty\":[", output);
    for (size_t i = 0; i < rows; ++i)
        fprintf(output, "%s%.9g", i ? "," : "", (double)novelty[i]);
    fputs("]}\n", output);
    int write_failed = ferror(output);
    if (fclose(output) || write_failed) fail("output write failed, artifact is incomplete");
    free(novelty);
    free(points);
    return 0;
}
