#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct wezterm_kit_session_t wezterm_kit_session_t;

enum {
  WEZTERM_KIT_STATUS_OK = 0,
  WEZTERM_KIT_STATUS_INVALID_ARGUMENT = 1,
  WEZTERM_KIT_STATUS_ALREADY_RUNNING = 2,
  WEZTERM_KIT_STATUS_NOT_RUNNING = 3,
  WEZTERM_KIT_STATUS_INTERNAL_ERROR = 255,
};

typedef struct wezterm_kit_size_t {
  uint16_t rows;
  uint16_t cols;
  uint16_t pixel_width;
  uint16_t pixel_height;
} wezterm_kit_size_t;

typedef struct wezterm_kit_env_var_t {
  const char *key;
  const char *value;
} wezterm_kit_env_var_t;

typedef void (*wezterm_kit_on_data_cb)(void *user_data, const uint8_t *bytes, size_t len);
typedef void (*wezterm_kit_on_string_cb)(void *user_data, const char *value);
typedef void (*wezterm_kit_on_bell_cb)(void *user_data);
typedef void (*wezterm_kit_on_exit_cb)(void *user_data, int32_t exit_code);
typedef void (*wezterm_kit_on_log_cb)(void *user_data, int32_t level, const char *message);

typedef struct wezterm_kit_callbacks_t {
  void *user_data;
  wezterm_kit_on_data_cb on_data;
  wezterm_kit_on_string_cb on_title;
  wezterm_kit_on_string_cb on_working_directory;
  wezterm_kit_on_bell_cb on_bell;
  wezterm_kit_on_exit_cb on_exit;
  wezterm_kit_on_log_cb on_log;
} wezterm_kit_callbacks_t;

typedef struct wezterm_kit_spawn_config_t {
  const char *program;
  const char *const *argv;
  size_t argc;
  const wezterm_kit_env_var_t *env;
  size_t env_count;
  const char *cwd;
  wezterm_kit_size_t size;
  bool clear_environment;
  bool controlling_tty;
} wezterm_kit_spawn_config_t;

uint32_t wezterm_kit_abi_version(void);
const char *wezterm_kit_status_string(int32_t status);

int32_t wezterm_kit_session_new(
    wezterm_kit_callbacks_t callbacks,
    wezterm_kit_session_t **out_session);

int32_t wezterm_kit_session_set_callbacks(
    wezterm_kit_session_t *session,
    wezterm_kit_callbacks_t callbacks);

void wezterm_kit_session_free(wezterm_kit_session_t *session);

int32_t wezterm_kit_session_spawn_local(
    wezterm_kit_session_t *session,
    const wezterm_kit_spawn_config_t *config);

int32_t wezterm_kit_session_write(
    wezterm_kit_session_t *session,
    const uint8_t *data,
    size_t len);

int32_t wezterm_kit_session_write_text(
    wezterm_kit_session_t *session,
    const char *text);

int32_t wezterm_kit_session_resize(
    wezterm_kit_session_t *session,
    wezterm_kit_size_t size);

int32_t wezterm_kit_session_kill(wezterm_kit_session_t *session);

#ifdef __cplusplus
}
#endif
