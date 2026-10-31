// Runtime validation for the two ANGLE features the Windows GL plan depends on:
//   1. A second EGL context can share a program with the presentation context.
//   2. EGL_ANDROID_blob_cache is exposed and callable on Windows/D3D11.
//
// This is intentionally a small, standalone Win32 executable. It loads the
// ANGLE DLLs from its own directory, creates a D3D11 display, compiles a
// program in a shared "compiler" context, and draws with that program from the
// main context. It also creates and deletes a file under the proposed
// %LOCALAPPDATA%\Cinnabar\cache\angle-probe location.
//
// Build with MSVC:
//   cl /EHsc /std:c++17 /O2 /Fe:angle_egl_probe.exe angle_egl_probe.cpp
//
// Run next to libEGL.dll, libGLESv2.dll and d3dcompiler_47.dll.

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

namespace {

using EGLBoolean = unsigned int;
using EGLint = int;
using EGLenum = unsigned int;
using EGLDisplay = void*;
using EGLConfig = void*;
using EGLContext = void*;
using EGLSurface = void*;
using EGLsizeiANDROID = int;

using GLenum = unsigned int;
using GLuint = unsigned int;
using GLint = int;
using GLsizei = int;
using GLboolean = unsigned char;
using GLchar = char;
using GLsizeiptr = std::ptrdiff_t;

constexpr EGLint EGL_NONE = 0x3038;
constexpr EGLint EGL_TRUE = 1;
constexpr EGLint EGL_SUCCESS = 0x3000;
constexpr EGLint EGL_EXTENSIONS = 0x3055;
constexpr EGLint EGL_VENDOR = 0x3053;
constexpr EGLint EGL_VERSION = 0x3054;
constexpr EGLint EGL_SURFACE_TYPE = 0x3033;
constexpr EGLint EGL_PBUFFER_BIT = 0x0001;
constexpr EGLint EGL_RENDERABLE_TYPE = 0x3040;
constexpr EGLint EGL_OPENGL_ES3_BIT = 0x00000040;
constexpr EGLint EGL_RED_SIZE = 0x3024;
constexpr EGLint EGL_GREEN_SIZE = 0x3023;
constexpr EGLint EGL_BLUE_SIZE = 0x3022;
constexpr EGLint EGL_ALPHA_SIZE = 0x3021;
constexpr EGLint EGL_DEPTH_SIZE = 0x3025;
constexpr EGLint EGL_WIDTH = 0x3057;
constexpr EGLint EGL_HEIGHT = 0x3056;
constexpr EGLint EGL_CONTEXT_CLIENT_VERSION = 0x3098;
constexpr EGLint EGL_CONTEXT_MAJOR_VERSION = 0x3098;
constexpr EGLint EGL_CONTEXT_MINOR_VERSION = 0x30FB;
constexpr EGLenum EGL_PLATFORM_ANGLE_ANGLE = 0x3202;
constexpr EGLint EGL_PLATFORM_ANGLE_TYPE_ANGLE = 0x3203;
constexpr EGLint EGL_PLATFORM_ANGLE_TYPE_D3D11_ANGLE = 0x3208;
constexpr void* EGL_DEFAULT_DISPLAY = nullptr;
constexpr EGLContext EGL_NO_CONTEXT = nullptr;
constexpr EGLSurface EGL_NO_SURFACE = nullptr;

constexpr GLenum GL_VERTEX_SHADER = 0x8B31;
constexpr GLenum GL_FRAGMENT_SHADER = 0x8B30;
constexpr GLenum GL_COMPILE_STATUS = 0x8B81;
constexpr GLenum GL_LINK_STATUS = 0x8B82;
constexpr GLenum GL_INFO_LOG_LENGTH = 0x8B84;
constexpr GLenum GL_ARRAY_BUFFER = 0x8892;
constexpr GLenum GL_STATIC_DRAW = 0x88E4;
constexpr GLenum GL_FLOAT = 0x1406;
constexpr GLenum GL_TRIANGLES = 0x0004;
constexpr GLenum GL_COLOR_BUFFER_BIT = 0x4000;
constexpr GLenum GL_RGBA = 0x1908;
constexpr GLenum GL_UNSIGNED_BYTE = 0x1401;
constexpr GLboolean GL_FALSE = 0;

using EGLSetBlobFunc = void(__stdcall*)(const void*, EGLsizeiANDROID, const void*, EGLsizeiANDROID);
using EGLGetBlobFunc = EGLsizeiANDROID(__stdcall*)(const void*, EGLsizeiANDROID, void*, EGLsizeiANDROID);

using PFNEGLGETPROCADDRESS = void*(__stdcall*)(const char*);
using PFNEGLGETPLATFORMDISPLAYEXT =
    EGLDisplay(__stdcall*)(EGLenum, void*, const EGLint*);
using PFNEGLINITIALIZE = EGLBoolean(__stdcall*)(EGLDisplay, EGLint*, EGLint*);
using PFNEGLQUERYSTRING = const char*(__stdcall*)(EGLDisplay, EGLint);
using PFNEGLCHOOSECONFIG =
    EGLBoolean(__stdcall*)(EGLDisplay, const EGLint*, EGLConfig*, EGLint, EGLint*);
using PFNEGLCREATECONTEXT = EGLContext(__stdcall*)(EGLDisplay, EGLConfig, EGLContext, const EGLint*);
using PFNEGLCREATEPBUFFERSURFACE = EGLSurface(__stdcall*)(EGLDisplay, EGLConfig, const EGLint*);
using PFNEGLMAKECURRENT = EGLBoolean(__stdcall*)(EGLDisplay, EGLSurface, EGLSurface, EGLContext);
using PFNEGLDESTROYCONTEXT = EGLBoolean(__stdcall*)(EGLDisplay, EGLContext);
using PFNEGLDESTROYSURFACE = EGLBoolean(__stdcall*)(EGLDisplay, EGLSurface);
using PFNEGLTERMINATE = EGLBoolean(__stdcall*)(EGLDisplay);
using PFNEGLGETERROR = EGLint(__stdcall*)();
using PFNEGLSETBLOBCACHEFUNCSANDROID =
    void(__stdcall*)(EGLDisplay, EGLSetBlobFunc, EGLGetBlobFunc);

using PFNGLCREATESHADER = GLuint(__stdcall*)(GLenum);
using PFNGLSHADERSOURCE =
    void(__stdcall*)(GLuint, GLsizei, const GLchar* const*, const GLint*);
using PFNGLCOMPILESHADER = void(__stdcall*)(GLuint);
using PFNGLGETSHADERIV = void(__stdcall*)(GLuint, GLenum, GLint*);
using PFNGLGETSHADERINFOLOG = void(__stdcall*)(GLuint, GLsizei, GLsizei*, GLchar*);
using PFNGLCREATEPROGRAM = GLuint(__stdcall*)();
using PFNGLATTACHSHADER = void(__stdcall*)(GLuint, GLuint);
using PFNGLLINKPROGRAM = void(__stdcall*)(GLuint);
using PFNGLGETPROGRAMIV = void(__stdcall*)(GLuint, GLenum, GLint*);
using PFNGLGETPROGRAMINFOLOG = void(__stdcall*)(GLuint, GLsizei, GLsizei*, GLchar*);
using PFNGLUSEPROGRAM = void(__stdcall*)(GLuint);
using PFNGLISPROGRAM = GLboolean(__stdcall*)(GLuint);
using PFNGLDELETESHADER = void(__stdcall*)(GLuint);
using PFNGLDELETEPROGRAM = void(__stdcall*)(GLuint);
using PFNGLGENVERTEXARRAYS = void(__stdcall*)(GLsizei, GLuint*);
using PFNGLBINDVERTEXARRAY = void(__stdcall*)(GLuint);
using PFNGLGENBUFFERS = void(__stdcall*)(GLsizei, GLuint*);
using PFNGLBINDBUFFER = void(__stdcall*)(GLenum, GLuint);
using PFNGLBUFFERDATA = void(__stdcall*)(GLenum, GLsizeiptr, const void*, GLenum);
using PFNGLGETATTRIBLOCATION = GLint(__stdcall*)(GLuint, const GLchar*);
using PFNGLENABLEVERTEXATTRIBARRAY = void(__stdcall*)(GLuint);
using PFNGLVERTEXATTRIBPOINTER =
    void(__stdcall*)(GLuint, GLint, GLenum, GLboolean, GLsizei, const void*);
using PFNGLVIEWPORT = void(__stdcall*)(GLint, GLint, GLsizei, GLsizei);
using PFNGLCLEARCOLOR = void(__stdcall*)(float, float, float, float);
using PFNGLCLEAR = void(__stdcall*)(GLenum);
using PFNGLDRAWARRAYS = void(__stdcall*)(GLenum, GLint, GLsizei);
using PFNGLFINISH = void(__stdcall*)();
using PFNGLREADPIXELS =
    void(__stdcall*)(GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, void*);
using PFNGLGETSTRING = const unsigned char*(__stdcall*)(GLenum);
using PFNGLGETERROR = GLenum(__stdcall*)();

struct Api {
    PFNEGLGETPROCADDRESS get_proc = nullptr;
    PFNEGLGETPLATFORMDISPLAYEXT get_platform_display = nullptr;
    PFNEGLINITIALIZE initialize = nullptr;
    PFNEGLQUERYSTRING query_string = nullptr;
    PFNEGLCHOOSECONFIG choose_config = nullptr;
    PFNEGLCREATECONTEXT create_context = nullptr;
    PFNEGLCREATEPBUFFERSURFACE create_pbuffer = nullptr;
    PFNEGLMAKECURRENT make_current = nullptr;
    PFNEGLDESTROYCONTEXT destroy_context = nullptr;
    PFNEGLDESTROYSURFACE destroy_surface = nullptr;
    PFNEGLTERMINATE terminate = nullptr;
    PFNEGLGETERROR get_error = nullptr;
    PFNEGLSETBLOBCACHEFUNCSANDROID set_blob_cache = nullptr;

    PFNGLCREATESHADER create_shader = nullptr;
    PFNGLSHADERSOURCE shader_source = nullptr;
    PFNGLCOMPILESHADER compile_shader = nullptr;
    PFNGLGETSHADERIV get_shader_iv = nullptr;
    PFNGLGETSHADERINFOLOG get_shader_log = nullptr;
    PFNGLCREATEPROGRAM create_program = nullptr;
    PFNGLATTACHSHADER attach_shader = nullptr;
    PFNGLLINKPROGRAM link_program = nullptr;
    PFNGLGETPROGRAMIV get_program_iv = nullptr;
    PFNGLGETPROGRAMINFOLOG get_program_log = nullptr;
    PFNGLUSEPROGRAM use_program = nullptr;
    PFNGLISPROGRAM is_program = nullptr;
    PFNGLDELETESHADER delete_shader = nullptr;
    PFNGLDELETEPROGRAM delete_program = nullptr;
    PFNGLGENVERTEXARRAYS gen_vertex_arrays = nullptr;
    PFNGLBINDVERTEXARRAY bind_vertex_array = nullptr;
    PFNGLGENBUFFERS gen_buffers = nullptr;
    PFNGLBINDBUFFER bind_buffer = nullptr;
    PFNGLBUFFERDATA buffer_data = nullptr;
    PFNGLGETATTRIBLOCATION get_attrib_location = nullptr;
    PFNGLENABLEVERTEXATTRIBARRAY enable_vertex_attrib = nullptr;
    PFNGLVERTEXATTRIBPOINTER vertex_attrib_pointer = nullptr;
    PFNGLVIEWPORT viewport = nullptr;
    PFNGLCLEARCOLOR clear_color = nullptr;
    PFNGLCLEAR clear = nullptr;
    PFNGLDRAWARRAYS draw_arrays = nullptr;
    PFNGLFINISH finish = nullptr;
    PFNGLREADPIXELS read_pixels = nullptr;
    PFNGLGETSTRING get_string = nullptr;
    PFNGLGETERROR get_gl_error = nullptr;
};

std::atomic<int> g_blob_sets{0};
std::atomic<int> g_blob_gets{0};

void __stdcall set_blob(const void*, EGLsizeiANDROID, const void*, EGLsizeiANDROID) {
    ++g_blob_sets;
}

EGLsizeiANDROID __stdcall get_blob(const void*, EGLsizeiANDROID, void*, EGLsizeiANDROID) {
    ++g_blob_gets;
    return 0;
}

void fail(const char* message) {
    std::fprintf(stderr, "FAIL: %s (egl_error=0x%04x)\n", message, 0u);
}

template <typename T>
T load_export(HMODULE module, const char* name) {
    auto* proc = GetProcAddress(module, name);
    if (proc == nullptr) {
        std::fprintf(stderr, "missing export: %s\n", name);
        return nullptr;
    }
    return reinterpret_cast<T>(proc);
}

template <typename T>
T load_egl(const Api& api, const char* name) {
    auto* proc = api.get_proc(name);
    if (proc == nullptr) {
        std::fprintf(stderr, "missing EGL proc: %s\n", name);
        return nullptr;
    }
    return reinterpret_cast<T>(proc);
}

bool load_gl(Api& api) {
    struct Entry {
        const char* name;
        void** target;
    };
    const Entry entries[] = {
        {"glCreateShader", reinterpret_cast<void**>(&api.create_shader)},
        {"glShaderSource", reinterpret_cast<void**>(&api.shader_source)},
        {"glCompileShader", reinterpret_cast<void**>(&api.compile_shader)},
        {"glGetShaderiv", reinterpret_cast<void**>(&api.get_shader_iv)},
        {"glGetShaderInfoLog", reinterpret_cast<void**>(&api.get_shader_log)},
        {"glCreateProgram", reinterpret_cast<void**>(&api.create_program)},
        {"glAttachShader", reinterpret_cast<void**>(&api.attach_shader)},
        {"glLinkProgram", reinterpret_cast<void**>(&api.link_program)},
        {"glGetProgramiv", reinterpret_cast<void**>(&api.get_program_iv)},
        {"glGetProgramInfoLog", reinterpret_cast<void**>(&api.get_program_log)},
        {"glUseProgram", reinterpret_cast<void**>(&api.use_program)},
        {"glIsProgram", reinterpret_cast<void**>(&api.is_program)},
        {"glDeleteShader", reinterpret_cast<void**>(&api.delete_shader)},
        {"glDeleteProgram", reinterpret_cast<void**>(&api.delete_program)},
        {"glGenVertexArrays", reinterpret_cast<void**>(&api.gen_vertex_arrays)},
        {"glBindVertexArray", reinterpret_cast<void**>(&api.bind_vertex_array)},
        {"glGenBuffers", reinterpret_cast<void**>(&api.gen_buffers)},
        {"glBindBuffer", reinterpret_cast<void**>(&api.bind_buffer)},
        {"glBufferData", reinterpret_cast<void**>(&api.buffer_data)},
        {"glGetAttribLocation", reinterpret_cast<void**>(&api.get_attrib_location)},
        {"glEnableVertexAttribArray", reinterpret_cast<void**>(&api.enable_vertex_attrib)},
        {"glVertexAttribPointer", reinterpret_cast<void**>(&api.vertex_attrib_pointer)},
        {"glViewport", reinterpret_cast<void**>(&api.viewport)},
        {"glClearColor", reinterpret_cast<void**>(&api.clear_color)},
        {"glClear", reinterpret_cast<void**>(&api.clear)},
        {"glDrawArrays", reinterpret_cast<void**>(&api.draw_arrays)},
        {"glFinish", reinterpret_cast<void**>(&api.finish)},
        {"glReadPixels", reinterpret_cast<void**>(&api.read_pixels)},
        {"glGetString", reinterpret_cast<void**>(&api.get_string)},
        {"glGetError", reinterpret_cast<void**>(&api.get_gl_error)},
    };
    for (const auto& entry : entries) {
        *entry.target = api.get_proc(entry.name);
        if (*entry.target == nullptr) {
            std::fprintf(stderr, "missing GL proc: %s\n", entry.name);
            return false;
        }
    }
    return true;
}

bool shader_ok(const Api& gl, GLuint shader, const char* label) {
    GLint ok = 0;
    gl.get_shader_iv(shader, GL_COMPILE_STATUS, &ok);
    if (ok != 0) {
        return true;
    }
    GLint length = 0;
    gl.get_shader_iv(shader, GL_INFO_LOG_LENGTH, &length);
    std::vector<GLchar> log(static_cast<size_t>(length > 1 ? length : 1), '\0');
    GLsizei written = 0;
    gl.get_shader_log(shader, static_cast<GLsizei>(log.size()), &written, log.data());
    std::fprintf(stderr, "shader %s failed: %s\n", label, log.data());
    return false;
}

bool program_ok(const Api& gl, GLuint program, const char* label) {
    GLint ok = 0;
    gl.get_program_iv(program, GL_LINK_STATUS, &ok);
    if (ok != 0) {
        return true;
    }
    GLint length = 0;
    gl.get_program_iv(program, GL_INFO_LOG_LENGTH, &length);
    std::vector<GLchar> log(static_cast<size_t>(length > 1 ? length : 1), '\0');
    GLsizei written = 0;
    gl.get_program_log(program, static_cast<GLsizei>(log.size()), &written, log.data());
    std::fprintf(stderr, "program %s failed: %s\n", label, log.data());
    return false;
}

bool cache_path_available(std::string& result) {
    wchar_t local_app_data[MAX_PATH] = {};
    const DWORD length =
        GetEnvironmentVariableW(L"LOCALAPPDATA", local_app_data, static_cast<DWORD>(MAX_PATH));
    if (length == 0 || length >= MAX_PATH) {
        result = "LOCALAPPDATA unavailable";
        return false;
    }

    const std::filesystem::path cache =
        std::filesystem::path(local_app_data) / L"Cinnabar" / L"cache" / L"angle-probe";
    std::error_code error;
    std::filesystem::create_directories(cache, error);
    if (error) {
        result = "cannot create " + cache.string() + ": " + error.message();
        return false;
    }

    const std::filesystem::path probe = cache / L"probe.txt";
    {
        std::ofstream out(probe, std::ios::binary | std::ios::trunc);
        if (!out) {
            result = "cannot write " + probe.string();
            return false;
        }
        out << "cinnabar angle cache probe\n";
    }
    std::filesystem::remove(probe, error);
    result = cache.string();
    return true;
}

int run() {
    std::string cache_result;
    const bool cache_path_ok = cache_path_available(cache_result);
    std::printf("local_app_data_cache_path=%s\n", cache_result.c_str());
    std::printf("local_app_data_cache_path_ok=%d\n", cache_path_ok ? 1 : 0);

    wchar_t module_path[MAX_PATH] = {};
    GetModuleFileNameW(nullptr, module_path, MAX_PATH);
    const std::filesystem::path module_dir = std::filesystem::path(module_path).parent_path();
    SetDllDirectoryW(module_dir.c_str());

    HMODULE gles = LoadLibraryA("libGLESv2.dll");
    if (gles == nullptr) {
        std::fprintf(stderr, "LoadLibrary(libGLESv2.dll) failed: %lu\n", GetLastError());
        return 2;
    }
    HMODULE egl = LoadLibraryA("libEGL.dll");
    if (egl == nullptr) {
        std::fprintf(stderr, "LoadLibrary(libEGL.dll) failed: %lu\n", GetLastError());
        return 2;
    }

    Api api;
    api.get_proc = load_export<PFNEGLGETPROCADDRESS>(egl, "eglGetProcAddress");
    api.initialize = load_export<PFNEGLINITIALIZE>(egl, "eglInitialize");
    api.query_string = load_export<PFNEGLQUERYSTRING>(egl, "eglQueryString");
    api.choose_config = load_export<PFNEGLCHOOSECONFIG>(egl, "eglChooseConfig");
    api.create_context = load_export<PFNEGLCREATECONTEXT>(egl, "eglCreateContext");
    api.create_pbuffer = load_export<PFNEGLCREATEPBUFFERSURFACE>(egl, "eglCreatePbufferSurface");
    api.make_current = load_export<PFNEGLMAKECURRENT>(egl, "eglMakeCurrent");
    api.destroy_context = load_export<PFNEGLDESTROYCONTEXT>(egl, "eglDestroyContext");
    api.destroy_surface = load_export<PFNEGLDESTROYSURFACE>(egl, "eglDestroySurface");
    api.terminate = load_export<PFNEGLTERMINATE>(egl, "eglTerminate");
    api.get_error = load_export<PFNEGLGETERROR>(egl, "eglGetError");
    if (api.get_proc == nullptr || api.initialize == nullptr || api.get_error == nullptr) {
        return 2;
    }

    api.get_platform_display =
        load_egl<PFNEGLGETPLATFORMDISPLAYEXT>(api, "eglGetPlatformDisplayEXT");
    api.set_blob_cache =
        load_egl<PFNEGLSETBLOBCACHEFUNCSANDROID>(api, "eglSetBlobCacheFuncsANDROID");
    if (!load_gl(api)) {
        return 2;
    }

    const EGLint display_attribs[] = {
        EGL_PLATFORM_ANGLE_TYPE_ANGLE,
        EGL_PLATFORM_ANGLE_TYPE_D3D11_ANGLE,
        EGL_NONE,
    };
    EGLDisplay display = api.get_platform_display
        ? api.get_platform_display(EGL_PLATFORM_ANGLE_ANGLE, EGL_DEFAULT_DISPLAY, display_attribs)
        : nullptr;
    if (display == nullptr) {
        std::fprintf(stderr, "eglGetPlatformDisplay(D3D11) failed: 0x%04x\n", api.get_error());
        return 2;
    }

    EGLint major = 0;
    EGLint minor = 0;
    if (api.initialize(display, &major, &minor) != EGL_TRUE) {
        std::fprintf(stderr, "eglInitialize failed: 0x%04x\n", api.get_error());
        return 2;
    }
    std::printf("egl_version=%d.%d\n", major, minor);
    const char* vendor = api.query_string(display, EGL_VENDOR);
    const char* version = api.query_string(display, EGL_VERSION);
    const char* extensions = api.query_string(display, EGL_EXTENSIONS);
    std::printf("egl_vendor=%s\n", vendor ? vendor : "(null)");
    std::printf("egl_client_version=%s\n", version ? version : "(null)");
    std::printf("egl_extensions=%s\n", extensions ? extensions : "(null)");
    const bool blob_advertised =
        extensions != nullptr && std::strstr(extensions, "EGL_ANDROID_blob_cache") != nullptr;
    std::printf("blob_cache_advertised=%d\n", blob_advertised ? 1 : 0);

    const EGLint config_attribs[] = {
        EGL_SURFACE_TYPE, EGL_PBUFFER_BIT,
        EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT,
        EGL_RED_SIZE, 8,
        EGL_GREEN_SIZE, 8,
        EGL_BLUE_SIZE, 8,
        EGL_ALPHA_SIZE, 8,
        EGL_DEPTH_SIZE, 24,
        EGL_NONE,
    };
    EGLConfig config = nullptr;
    EGLint config_count = 0;
    if (api.choose_config(display, config_attribs, &config, 1, &config_count) != EGL_TRUE ||
        config_count == 0) {
        std::fprintf(stderr, "eglChooseConfig failed: 0x%04x\n", api.get_error());
        return 2;
    }

    const EGLint pbuffer_attribs[] = {EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE};
    EGLSurface main_surface = api.create_pbuffer(display, config, pbuffer_attribs);
    EGLSurface compiler_surface = api.create_pbuffer(display, config, pbuffer_attribs);
    if (main_surface == EGL_NO_SURFACE || compiler_surface == EGL_NO_SURFACE) {
        std::fprintf(stderr, "eglCreatePbufferSurface failed: 0x%04x\n", api.get_error());
        return 2;
    }

    const EGLint context_attribs[] = {
        EGL_CONTEXT_MAJOR_VERSION, 3,
        EGL_CONTEXT_MINOR_VERSION, 1,
        EGL_NONE,
    };
    EGLContext main_context =
        api.create_context(display, config, EGL_NO_CONTEXT, context_attribs);
    if (main_context == EGL_NO_CONTEXT) {
        std::fprintf(stderr, "main eglCreateContext failed: 0x%04x\n", api.get_error());
        return 2;
    }

    EGLContext compiler_context =
        api.create_context(display, config, main_context, context_attribs);
    if (compiler_context == EGL_NO_CONTEXT) {
        std::fprintf(stderr, "shared eglCreateContext failed: 0x%04x\n", api.get_error());
        return 2;
    }
    std::printf("shared_context_created=1\n");

    if (api.make_current(display, compiler_surface, compiler_surface, compiler_context) != EGL_TRUE) {
        std::fprintf(stderr, "make_current(compiler) failed: 0x%04x\n", api.get_error());
        return 2;
    }

    if (api.set_blob_cache != nullptr) {
        api.set_blob_cache(display, set_blob, get_blob);
        const EGLint error = api.get_error();
        std::printf("set_blob_cache_error=0x%04x\n", error);
    } else {
        std::printf("set_blob_cache_error=missing\n");
    }

    const char* renderer = reinterpret_cast<const char*>(api.get_string(0x1F01)); // GL_RENDERER
    const char* gl_version = reinterpret_cast<const char*>(api.get_string(0x1F02)); // GL_VERSION
    std::printf("gl_renderer=%s\n", renderer ? renderer : "(null)");
    std::printf("gl_version=%s\n", gl_version ? gl_version : "(null)");

    const char* vertex_source =
        "#version 300 es\n"
        "in vec2 aPos;\n"
        "void main() { gl_Position = vec4(aPos, 0.0, 1.0); }\n";
    const char* fragment_source =
        "#version 300 es\n"
        "precision mediump float;\n"
        "out vec4 color;\n"
        "void main() { color = vec4(1.0, 0.0, 0.0, 1.0); }\n";

    GLuint vertex = api.create_shader(GL_VERTEX_SHADER);
    GLuint fragment = api.create_shader(GL_FRAGMENT_SHADER);
    api.shader_source(vertex, 1, &vertex_source, nullptr);
    api.shader_source(fragment, 1, &fragment_source, nullptr);
    api.compile_shader(vertex);
    api.compile_shader(fragment);
    if (!shader_ok(api, vertex, "vertex") || !shader_ok(api, fragment, "fragment")) {
        return 3;
    }

    GLuint program = api.create_program();
    api.attach_shader(program, vertex);
    api.attach_shader(program, fragment);
    api.link_program(program);
    if (!program_ok(api, program, "compiler context")) {
        return 3;
    }
    api.delete_shader(vertex);
    api.delete_shader(fragment);
    std::printf("program_linked_in_compiler_context=1\n");

    if (api.make_current(display, main_surface, main_surface, main_context) != EGL_TRUE) {
        std::fprintf(stderr, "make_current(main) failed: 0x%04x\n", api.get_error());
        return 3;
    }
    const GLboolean visible = api.is_program(program);
    api.use_program(program);
    GLint linked = 0;
    api.get_program_iv(program, GL_LINK_STATUS, &linked);
    std::printf("program_visible_in_main=%d\n", visible ? 1 : 0);
    std::printf("program_linked_in_main=%d\n", linked ? 1 : 0);

    const GLfloat vertices[] = {-1.0f, -1.0f, 3.0f, -1.0f, -1.0f, 3.0f};
    GLuint vao = 0;
    GLuint buffer = 0;
    api.gen_vertex_arrays(1, &vao);
    api.bind_vertex_array(vao);
    api.gen_buffers(1, &buffer);
    api.bind_buffer(GL_ARRAY_BUFFER, buffer);
    api.buffer_data(GL_ARRAY_BUFFER, sizeof(vertices), vertices, GL_STATIC_DRAW);
    const GLint location = api.get_attrib_location(program, "aPos");
    if (location >= 0) {
        api.enable_vertex_attrib(static_cast<GLuint>(location));
        api.vertex_attrib_pointer(
            static_cast<GLuint>(location), 2, GL_FLOAT, GL_FALSE, 0, nullptr);
    }
    api.viewport(0, 0, 16, 16);
    api.clear_color(0.0f, 0.0f, 0.0f, 1.0f);
    api.clear(GL_COLOR_BUFFER_BIT);
    if (location >= 0) {
        api.draw_arrays(GL_TRIANGLES, 0, 3);
    }
    api.finish();
    unsigned char pixel[4] = {};
    api.read_pixels(8, 8, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel);
    const bool red = pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50;
    std::printf("pixel=%u,%u,%u,%u\n", pixel[0], pixel[1], pixel[2], pixel[3]);
    std::printf("shared_program_draw_ok=%d\n", red && location >= 0 ? 1 : 0);
    std::printf("blob_cache_set_calls=%d\n", g_blob_sets.load());
    std::printf("blob_cache_get_calls=%d\n", g_blob_gets.load());

    api.make_current(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    api.destroy_context(display, compiler_context);
    api.destroy_context(display, main_context);
    api.destroy_surface(display, compiler_surface);
    api.destroy_surface(display, main_surface);
    api.terminate(display);

    const bool ok = blob_advertised && red && location >= 0 && visible && linked &&
                    cache_path_ok && api.set_blob_cache != nullptr;
    std::printf("RESULT shared_context=%s blob_cache=%s cache_path=%s\n",
                red && location >= 0 && visible && linked ? "ok" : "failed",
                blob_advertised && api.set_blob_cache != nullptr ? "ok" : "failed",
                cache_path_ok ? "ok" : "failed");
    return ok ? 0 : 4;
}

} // namespace

int main() {
    const int result = run();
    std::printf("exit_code=%d\n", result);
    return result;
}
