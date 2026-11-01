// Native Direct3D 11 feasibility probe for Cinnabar's Windows renderer.
//
// The probe has no Rust or wgpu dependency. It:
//   1. creates a hidden Win32 window;
//   2. creates a D3D11 device and, when possible, a BGRA swap chain;
//   3. compiles a tiny HLSL vertex/pixel shader pair with D3DCompile;
//   4. draws a red triangle into a 64x64 offscreen target and reads it back;
//   5. presents a clear color through the swap chain.
//
// If window or swap-chain creation fails (for example on a non-interactive CI
// desktop), it retries with a device-only WARP path and still runs the
// offscreen checks. The log states which path ran. The exit code is non-zero
// when the device, BGRA render-target, triangle, or readback check fails, or
// when a swap chain exists but Present fails.

#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <dxgi.h>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "d3dcompiler.lib")
#pragma comment(lib, "dxgi.lib")
#pragma comment(lib, "user32.lib")

namespace {

constexpr wchar_t kWindowClass[] = L"CinnabarD3D11ProbeWindow";
constexpr wchar_t kWindowTitle[] = L"Cinnabar D3D11 probe";
constexpr UINT kWindowWidth = 320;
constexpr UINT kWindowHeight = 240;
constexpr UINT kProbeSize = 64;
constexpr std::size_t kBytesPerPixel = 4;
constexpr UINT kDeviceFlags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
constexpr D3D_FEATURE_LEVEL kFeatureLevels[] = {
    D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_10_1,
};

// This triangle covers the center pixel of the 64x64 target. Its vertices are
// clockwise in D3D screen space so the default back-face culling keeps it. The
// pixel shader returns a constant red so the readback expectation is
// unambiguous.
constexpr char kTriangleShader[] = R"hlsl(
struct VertexOutput {
    float4 position : SV_Position;
    float4 color : COLOR0;
};

VertexOutput VSMain(uint vertexId : SV_VertexID) {
    float2 position = float2(0.8, -0.8);
    if (vertexId == 0) {
        position = float2(-0.8, -0.8);
    } else if (vertexId == 1) {
        position = float2(0.0, 0.8);
    }

    VertexOutput output;
    output.position = float4(position, 0.0, 1.0);
    output.color = float4(1.0, 0.0, 0.0, 1.0);
    return output;
}

float4 PSMain(float4 color : COLOR0) : SV_Target {
    return color;
}
)hlsl";

void Log(const std::string& message) {
    std::cout << message << '\n';
    std::cout.flush();
}

std::string HexFixed(std::uint32_t value, int width) {
    std::ostringstream output;
    output << "0x" << std::hex << std::uppercase << std::setfill('0')
           << std::setw(width) << value;
    return output.str();
}

std::string FormatHr(HRESULT result) {
    return HexFixed(static_cast<std::uint32_t>(result), 8);
}

const char* BoolName(bool value) {
    return value ? "yes" : "no";
}

const char* FeatureLevelName(D3D_FEATURE_LEVEL level) {
    switch (level) {
        case D3D_FEATURE_LEVEL_11_1:
            return "11_1";
        case D3D_FEATURE_LEVEL_11_0:
            return "11_0";
        case D3D_FEATURE_LEVEL_10_1:
            return "10_1";
        case D3D_FEATURE_LEVEL_10_0:
            return "10_0";
        case D3D_FEATURE_LEVEL_9_3:
            return "9_3";
        case D3D_FEATURE_LEVEL_9_2:
            return "9_2";
        case D3D_FEATURE_LEVEL_9_1:
            return "9_1";
        default:
            return "unknown";
    }
}

const char* DriverTypeName(D3D_DRIVER_TYPE type) {
    switch (type) {
        case D3D_DRIVER_TYPE_HARDWARE:
            return "hardware";
        case D3D_DRIVER_TYPE_WARP:
            return "warp";
        case D3D_DRIVER_TYPE_REFERENCE:
            return "reference";
        case D3D_DRIVER_TYPE_NULL:
            return "null";
        case D3D_DRIVER_TYPE_SOFTWARE:
            return "software";
        case D3D_DRIVER_TYPE_UNKNOWN:
            return "unknown";
        default:
            return "other";
    }
}

std::string Utf8FromWide(const wchar_t* text) {
    if (text == nullptr || text[0] == L'\0') {
        return {};
    }

    const int size = WideCharToMultiByte(
        CP_UTF8, 0, text, -1, nullptr, 0, nullptr, nullptr);
    if (size <= 0) {
        return {};
    }

    std::vector<char> buffer(static_cast<std::size_t>(size));
    const int written = WideCharToMultiByte(
        CP_UTF8, 0, text, -1, buffer.data(), size, nullptr, nullptr);
    if (written <= 0) {
        return {};
    }
    return std::string(buffer.data(), static_cast<std::size_t>(written - 1));
}

std::string Win32Error(DWORD code) {
    wchar_t* buffer = nullptr;
    const DWORD length = FormatMessageW(
        FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
            FORMAT_MESSAGE_IGNORE_INSERTS,
        nullptr, code, MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT),
        reinterpret_cast<LPWSTR>(&buffer), 0, nullptr);

    std::string text = length != 0 && buffer != nullptr
        ? Utf8FromWide(buffer)
        : std::string("unknown error");
    if (buffer != nullptr) {
        LocalFree(buffer);
    }
    while (!text.empty() &&
           (text.back() == '\r' || text.back() == '\n' ||
            text.back() == ' ' || text.back() == '\t')) {
        text.pop_back();
    }
    return text + " (win32=" + std::to_string(code) + ")";
}

void LogModulePath(const char* label, const wchar_t* module_name) {
    const HMODULE module = GetModuleHandleW(module_name);
    if (module == nullptr) {
        Log(std::string("[warn] ") + label + ": module is not loaded");
        return;
    }

    wchar_t path[MAX_PATH] = {};
    const DWORD length = GetModuleFileNameW(
        module, path, static_cast<DWORD>(ARRAYSIZE(path)));
    if (length == 0) {
        Log(std::string("[warn] ") + label + ": GetModuleFileNameW failed: " +
            Win32Error(GetLastError()));
        return;
    }
    Log(std::string("[info] ") + label + ": " + Utf8FromWide(path));
}

template <typename T>
class ComPtr {
public:
    ComPtr() = default;
    ~ComPtr() { Reset(); }

    ComPtr(const ComPtr&) = delete;
    ComPtr& operator=(const ComPtr&) = delete;

    T* Get() const { return pointer_; }
    T* const* GetAddressOf() const { return &pointer_; }
    T** ReleaseAndGetAddressOf() {
        Reset();
        return &pointer_;
    }
    T* operator->() const { return pointer_; }
    explicit operator bool() const { return pointer_ != nullptr; }

    void Reset() {
        if (pointer_ != nullptr) {
            pointer_->Release();
            pointer_ = nullptr;
        }
    }

    void Attach(T* pointer) {
        Reset();
        pointer_ = pointer;
    }

    T* Detach() {
        T* pointer = pointer_;
        pointer_ = nullptr;
        return pointer;
    }

private:
    T* pointer_ = nullptr;
};

struct DeviceState {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGISwapChain> swap_chain;
    ComPtr<ID3D11RenderTargetView> swap_chain_view;
    D3D_FEATURE_LEVEL feature_level = D3D_FEATURE_LEVEL_9_1;
    D3D_DRIVER_TYPE driver_type = D3D_DRIVER_TYPE_UNKNOWN;
    bool using_swap_chain = false;
};

bool IsAcceptedFeatureLevel(D3D_FEATURE_LEVEL level) {
    return level == D3D_FEATURE_LEVEL_11_0 ||
           level == D3D_FEATURE_LEVEL_10_1;
}

void LogAdapter(ID3D11Device* device) {
    ComPtr<IDXGIDevice> dxgi_device;
    HRESULT result = device->QueryInterface(
        __uuidof(IDXGIDevice),
        reinterpret_cast<void**>(dxgi_device.ReleaseAndGetAddressOf()));
    if (FAILED(result)) {
        Log("[warn] adapter: IDXGIDevice query failed: " + FormatHr(result));
        return;
    }

    ComPtr<IDXGIAdapter> adapter;
    result = dxgi_device->GetAdapter(adapter.ReleaseAndGetAddressOf());
    if (FAILED(result) || !adapter) {
        Log("[warn] adapter: IDXGIDevice::GetAdapter failed: " +
            FormatHr(result));
        return;
    }

    DXGI_ADAPTER_DESC desc = {};
    result = adapter->GetDesc(&desc);
    if (FAILED(result)) {
        Log("[warn] adapter: IDXGIAdapter::GetDesc failed: " +
            FormatHr(result));
        return;
    }
    Log("[info] adapter: " + Utf8FromWide(desc.Description));

    ComPtr<IDXGIAdapter1> adapter1;
    DXGI_ADAPTER_DESC1 desc1 = {};
    bool has_desc1 = false;
    result = adapter->QueryInterface(
        __uuidof(IDXGIAdapter1),
        reinterpret_cast<void**>(adapter1.ReleaseAndGetAddressOf()));
    if (SUCCEEDED(result) && adapter1) {
        result = adapter1->GetDesc1(&desc1);
        has_desc1 = SUCCEEDED(result);
    }

    constexpr std::uint64_t kMebibyte = 1024ull * 1024ull;
    std::ostringstream details;
    details << "[info] adapter: vendor=" << HexFixed(desc.VendorId, 4)
            << " device=" << HexFixed(desc.DeviceId, 4)
            << " subsystem=" << HexFixed(desc.SubSystemId, 8)
            << " revision=" << HexFixed(desc.Revision, 4)
            << " dedicated_video="
            << (static_cast<std::uint64_t>(desc.DedicatedVideoMemory) /
                kMebibyte)
            << " MiB"
            << " dedicated_system="
            << (static_cast<std::uint64_t>(desc.DedicatedSystemMemory) /
                kMebibyte)
            << " MiB"
            << " shared_system="
            << (static_cast<std::uint64_t>(desc.SharedSystemMemory) /
                kMebibyte)
            << " MiB";
    if (has_desc1) {
        details << " flags=" << HexFixed(desc1.Flags, 8);
    }
    Log(details.str());
}

HWND CreateProbeWindow(HINSTANCE instance) {
    WNDCLASSEXW window_class = {};
    window_class.cbSize = static_cast<UINT>(sizeof(window_class));
    window_class.lpfnWndProc = DefWindowProcW;
    window_class.hInstance = instance;
    window_class.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    window_class.lpszClassName = kWindowClass;

    if (RegisterClassExW(&window_class) == 0) {
        const DWORD error = GetLastError();
        if (error != ERROR_CLASS_ALREADY_EXISTS) {
            Log("[warn] window: RegisterClassExW failed: " +
                Win32Error(error));
            return nullptr;
        }
    }

    HWND window = CreateWindowExW(
        0, kWindowClass, kWindowTitle,
        static_cast<DWORD>(WS_OVERLAPPEDWINDOW),
        CW_USEDEFAULT, CW_USEDEFAULT,
        static_cast<int>(kWindowWidth), static_cast<int>(kWindowHeight),
        nullptr, nullptr, instance, nullptr);
    if (window == nullptr) {
        Log("[warn] window: CreateWindowExW failed: " +
            Win32Error(GetLastError()));
        return nullptr;
    }

    // No ShowWindow call: the window stays hidden on interactive desktops.
    Log("[pass] window: created hidden Win32 window");
    return window;
}

bool CreateSwapChainDevice(
    HWND window, D3D_DRIVER_TYPE driver_type, DeviceState* state) {
    Log("[info] device: trying D3D11CreateDeviceAndSwapChain (" +
        std::string(DriverTypeName(driver_type)) + ")");

    DXGI_SWAP_CHAIN_DESC swap_chain_desc = {};
    swap_chain_desc.BufferDesc.Width = 0;
    swap_chain_desc.BufferDesc.Height = 0;
    swap_chain_desc.BufferDesc.RefreshRate.Numerator = 60;
    swap_chain_desc.BufferDesc.RefreshRate.Denominator = 1;
    swap_chain_desc.BufferDesc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    swap_chain_desc.SampleDesc.Count = 1;
    swap_chain_desc.SampleDesc.Quality = 0;
    swap_chain_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    swap_chain_desc.BufferCount = 1;
    swap_chain_desc.OutputWindow = window;
    swap_chain_desc.Windowed = TRUE;
    swap_chain_desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    swap_chain_desc.Flags = 0;

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGISwapChain> swap_chain;
    D3D_FEATURE_LEVEL feature_level = D3D_FEATURE_LEVEL_9_1;

    HRESULT result = D3D11CreateDeviceAndSwapChain(
        nullptr, driver_type, nullptr, kDeviceFlags,
        kFeatureLevels, static_cast<UINT>(ARRAYSIZE(kFeatureLevels)),
        D3D11_SDK_VERSION, &swap_chain_desc,
        swap_chain.ReleaseAndGetAddressOf(),
        device.ReleaseAndGetAddressOf(), &feature_level,
        context.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[warn] device: D3D11CreateDeviceAndSwapChain (" +
            std::string(DriverTypeName(driver_type)) + ") failed: " +
            FormatHr(result));
        return false;
    }
    if (!device || !context || !swap_chain) {
        Log("[warn] device: D3D11CreateDeviceAndSwapChain returned "
            "incomplete objects");
        return false;
    }
    if (!IsAcceptedFeatureLevel(feature_level)) {
        Log("[warn] device: unsupported feature level " +
            std::string(FeatureLevelName(feature_level)));
        return false;
    }

    DXGI_SWAP_CHAIN_DESC actual_desc = {};
    result = swap_chain->GetDesc(&actual_desc);
    if (FAILED(result)) {
        Log("[warn] swapchain: GetDesc failed: " + FormatHr(result));
        return false;
    }
    if (actual_desc.BufferDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM) {
        Log("[warn] swapchain: unexpected format " +
            std::to_string(
                static_cast<unsigned int>(actual_desc.BufferDesc.Format)));
        return false;
    }

    ComPtr<ID3D11Texture2D> back_buffer;
    result = swap_chain->GetBuffer(
        0, __uuidof(ID3D11Texture2D),
        reinterpret_cast<void**>(back_buffer.ReleaseAndGetAddressOf()));
    if (FAILED(result) || !back_buffer) {
        Log("[warn] swapchain: GetBuffer failed: " + FormatHr(result));
        return false;
    }

    ComPtr<ID3D11RenderTargetView> swap_chain_view;
    result = device->CreateRenderTargetView(
        back_buffer.Get(), nullptr,
        swap_chain_view.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[warn] swapchain: CreateRenderTargetView failed: " +
            FormatHr(result));
        return false;
    }

    state->device.Attach(device.Detach());
    state->context.Attach(context.Detach());
    state->swap_chain.Attach(swap_chain.Detach());
    state->swap_chain_view.Attach(swap_chain_view.Detach());
    state->feature_level = feature_level;
    state->driver_type = driver_type;
    state->using_swap_chain = true;

    Log("[pass] swapchain: " +
        std::to_string(actual_desc.BufferDesc.Width) + "x" +
        std::to_string(actual_desc.BufferDesc.Height) +
        " BGRA8_UNORM, buffers=" +
        std::to_string(actual_desc.BufferCount));
    return true;
}

bool CreateOffscreenDevice(
    D3D_DRIVER_TYPE driver_type, DeviceState* state) {
    Log("[info] device: trying D3D11CreateDevice (" +
        std::string(DriverTypeName(driver_type)) + ")");

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    D3D_FEATURE_LEVEL feature_level = D3D_FEATURE_LEVEL_9_1;

    HRESULT result = D3D11CreateDevice(
        nullptr, driver_type, nullptr, kDeviceFlags,
        kFeatureLevels, static_cast<UINT>(ARRAYSIZE(kFeatureLevels)),
        D3D11_SDK_VERSION, device.ReleaseAndGetAddressOf(), &feature_level,
        context.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[warn] device: D3D11CreateDevice (" +
            std::string(DriverTypeName(driver_type)) + ") failed: " +
            FormatHr(result));
        return false;
    }
    if (!device || !context) {
        Log("[warn] device: D3D11CreateDevice returned incomplete objects");
        return false;
    }
    if (!IsAcceptedFeatureLevel(feature_level)) {
        Log("[warn] device: unsupported feature level " +
            std::string(FeatureLevelName(feature_level)));
        return false;
    }

    state->device.Attach(device.Detach());
    state->context.Attach(context.Detach());
    state->swap_chain.Reset();
    state->swap_chain_view.Reset();
    state->feature_level = feature_level;
    state->driver_type = driver_type;
    state->using_swap_chain = false;
    return true;
}

bool CheckBgraSupport(ID3D11Device* device) {
    UINT support = 0;
    const HRESULT result = device->CheckFormatSupport(
        DXGI_FORMAT_B8G8R8A8_UNORM, &support);
    if (FAILED(result)) {
        Log("[fail] BGRA8: CheckFormatSupport failed: " + FormatHr(result));
        return false;
    }

    const bool render_target =
        (support & D3D11_FORMAT_SUPPORT_RENDER_TARGET) != 0;
    const bool blendable =
        (support & D3D11_FORMAT_SUPPORT_BLENDABLE) != 0;
    const bool texture2d =
        (support & D3D11_FORMAT_SUPPORT_TEXTURE2D) != 0;

    Log("[info] BGRA8 support: mask=" + HexFixed(support, 8) +
        " render_target=" + BoolName(render_target) +
        " blendable=" + BoolName(blendable) +
        " texture2d=" + BoolName(texture2d));
    if (!render_target) {
        Log("[fail] BGRA8: render-target usage is not supported");
        return false;
    }
    Log("[pass] BGRA8: render-target support");
    return true;
}

bool CompileShader(
    const char* entry_point, const char* target, ComPtr<ID3DBlob>* blob) {
    ComPtr<ID3DBlob> code;
    ComPtr<ID3DBlob> errors;
    const HRESULT result = D3DCompile(
        kTriangleShader, std::strlen(kTriangleShader), "d3d11_probe.hlsl",
        nullptr, nullptr, entry_point, target,
        D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3, 0,
        code.ReleaseAndGetAddressOf(), errors.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[fail] shader: D3DCompile " + std::string(entry_point) + " (" +
            target + ") failed: " + FormatHr(result));
        if (errors) {
            std::string message(
                static_cast<const char*>(errors->GetBufferPointer()),
                errors->GetBufferSize());
            while (!message.empty() &&
                   (message.back() == '\0' || message.back() == '\r' ||
                    message.back() == '\n')) {
                message.pop_back();
            }
            Log("[info] shader compiler output:");
            Log(message);
        }
        return false;
    }
    if (!code) {
        Log("[fail] shader: D3DCompile returned no bytecode");
        return false;
    }

    Log("[pass] shader: compiled " + std::string(entry_point) + " (" +
        target + ", " + std::to_string(code->GetBufferSize()) + " bytes)");
    blob->Attach(code.Detach());
    return true;
}

bool CreateShaders(
    ID3D11Device* device, ComPtr<ID3D11VertexShader>* vertex_shader,
    ComPtr<ID3D11PixelShader>* pixel_shader) {
    ComPtr<ID3DBlob> vertex_bytecode;
    ComPtr<ID3DBlob> pixel_bytecode;
    if (!CompileShader("VSMain", "vs_5_0", &vertex_bytecode)) {
        return false;
    }
    if (!CompileShader("PSMain", "ps_5_0", &pixel_bytecode)) {
        return false;
    }

    HRESULT result = device->CreateVertexShader(
        vertex_bytecode->GetBufferPointer(), vertex_bytecode->GetBufferSize(),
        nullptr, vertex_shader->ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[fail] shader: CreateVertexShader failed: " + FormatHr(result));
        return false;
    }
    result = device->CreatePixelShader(
        pixel_bytecode->GetBufferPointer(), pixel_bytecode->GetBufferSize(),
        nullptr, pixel_shader->ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[fail] shader: CreatePixelShader failed: " + FormatHr(result));
        return false;
    }

    Log("[pass] shader: vertex and pixel stage objects created");
    return true;
}

bool RunOffscreenTriangle(
    ID3D11Device* device, ID3D11DeviceContext* context) {
    D3D11_TEXTURE2D_DESC target_desc = {};
    target_desc.Width = kProbeSize;
    target_desc.Height = kProbeSize;
    target_desc.MipLevels = 1;
    target_desc.ArraySize = 1;
    target_desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    target_desc.SampleDesc.Count = 1;
    target_desc.SampleDesc.Quality = 0;
    target_desc.Usage = D3D11_USAGE_DEFAULT;
    target_desc.BindFlags = D3D11_BIND_RENDER_TARGET;
    target_desc.CPUAccessFlags = 0;
    target_desc.MiscFlags = 0;

    ComPtr<ID3D11Texture2D> target;
    HRESULT result = device->CreateTexture2D(
        &target_desc, nullptr, target.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[fail] offscreen: CreateTexture2D (target) failed: " +
            FormatHr(result));
        return false;
    }

    ComPtr<ID3D11RenderTargetView> target_view;
    result = device->CreateRenderTargetView(
        target.Get(), nullptr, target_view.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[fail] offscreen: CreateRenderTargetView failed: " +
            FormatHr(result));
        return false;
    }

    D3D11_TEXTURE2D_DESC staging_desc = target_desc;
    staging_desc.Usage = D3D11_USAGE_STAGING;
    staging_desc.BindFlags = 0;
    staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging;
    result = device->CreateTexture2D(
        &staging_desc, nullptr, staging.ReleaseAndGetAddressOf());
    if (FAILED(result)) {
        Log("[fail] offscreen: CreateTexture2D (staging) failed: " +
            FormatHr(result));
        return false;
    }

    Log("[pass] offscreen: 64x64 BGRA8_UNORM render target and staging "
        "texture");

    ComPtr<ID3D11VertexShader> vertex_shader;
    ComPtr<ID3D11PixelShader> pixel_shader;
    if (!CreateShaders(device, &vertex_shader, &pixel_shader)) {
        return false;
    }

    const float clear_color[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    context->ClearRenderTargetView(target_view.Get(), clear_color);

    ID3D11RenderTargetView* views[] = {target_view.Get()};
    context->OMSetRenderTargets(1, views, nullptr);

    D3D11_VIEWPORT viewport = {};
    viewport.TopLeftX = 0.0f;
    viewport.TopLeftY = 0.0f;
    viewport.Width = static_cast<float>(kProbeSize);
    viewport.Height = static_cast<float>(kProbeSize);
    viewport.MinDepth = 0.0f;
    viewport.MaxDepth = 1.0f;
    context->RSSetViewports(1, &viewport);

    context->IASetInputLayout(nullptr);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->VSSetShader(vertex_shader.Get(), nullptr, 0);
    context->PSSetShader(pixel_shader.Get(), nullptr, 0);
    context->Draw(3, 0);

    // The render target cannot be a copy source while it is bound.
    context->OMSetRenderTargets(0, nullptr, nullptr);
    context->CopyResource(staging.Get(), target.Get());

    D3D11_MAPPED_SUBRESOURCE mapped = {};
    result = context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped);
    if (FAILED(result)) {
        Log("[fail] readback: Map failed: " + FormatHr(result));
        return false;
    }

    const std::size_t min_row_pitch = static_cast<std::size_t>(kProbeSize) *
        kBytesPerPixel;
    if (static_cast<std::size_t>(mapped.RowPitch) < min_row_pitch) {
        context->Unmap(staging.Get(), 0);
        Log("[fail] readback: staging row pitch is too small");
        return false;
    }

    const std::size_t center = static_cast<std::size_t>(kProbeSize / 2);
    const std::uint8_t* row =
        static_cast<const std::uint8_t*>(mapped.pData) +
        center * static_cast<std::size_t>(mapped.RowPitch);
    const std::uint8_t* pixel = row + center * kBytesPerPixel;
    const std::uint8_t blue = pixel[0];
    const std::uint8_t green = pixel[1];
    const std::uint8_t red = pixel[2];
    const std::uint8_t alpha = pixel[3];
    context->Unmap(staging.Get(), 0);

    std::ostringstream pixel_log;
    pixel_log << "[info] readback: pixel=(" << center << "," << center
              << ") BGRA=(" << static_cast<unsigned int>(blue) << ","
              << static_cast<unsigned int>(green) << ","
              << static_cast<unsigned int>(red) << ","
              << static_cast<unsigned int>(alpha) << ")";
    Log(pixel_log.str());

    const bool is_red =
        red >= 250 && green <= 5 && blue <= 5 && alpha >= 250;
    if (!is_red) {
        Log("[fail] readback: expected a red pixel, got BGRA=(" +
            std::to_string(static_cast<unsigned int>(blue)) + "," +
            std::to_string(static_cast<unsigned int>(green)) + "," +
            std::to_string(static_cast<unsigned int>(red)) + "," +
            std::to_string(static_cast<unsigned int>(alpha)) + ")");
        return false;
    }

    Log("[pass] readback: red pixel verified");
    return true;
}

bool PresentClear(
    ID3D11DeviceContext* context, IDXGISwapChain* swap_chain,
    ID3D11RenderTargetView* swap_chain_view) {
    const float clear_color[4] = {0.05f, 0.20f, 0.55f, 1.0f};
    context->ClearRenderTargetView(swap_chain_view, clear_color);
    Log("[info] present: calling Present(0, 0) after clear");

    const HRESULT result = swap_chain->Present(0, 0);
    Log("[info] present: hr=" + FormatHr(result));
    if (FAILED(result)) {
        Log("[fail] present: Present failed: " + FormatHr(result));
        return false;
    }
    if (result == DXGI_STATUS_OCCLUDED) {
        Log("[pass] present: clear submitted (window occluded)");
    } else {
        Log("[pass] present: clear submitted");
    }
    return true;
}

int RunProbe(HWND window) {
    DeviceState state;
    const D3D_DRIVER_TYPE drivers[] = {
        D3D_DRIVER_TYPE_HARDWARE,
        D3D_DRIVER_TYPE_WARP,
    };

    if (window != nullptr) {
        for (const D3D_DRIVER_TYPE driver : drivers) {
            if (CreateSwapChainDevice(window, driver, &state)) {
                break;
            }
        }
    } else {
        Log("[warn] window: unavailable; using offscreen-only path");
    }

    if (!state.device) {
        if (window != nullptr) {
            Log("[warn] swapchain: unavailable; falling back to device-only "
                "offscreen path");
        }
        for (const D3D_DRIVER_TYPE driver : drivers) {
            if (CreateOffscreenDevice(driver, &state)) {
                break;
            }
        }
    }

    if (!state.device || !state.context) {
        Log("[fail] RESULT: FAIL (no D3D11 device)");
        return 1;
    }

    Log("[pass] device: driver=" + std::string(DriverTypeName(
        state.driver_type)) + " feature_level=" +
        FeatureLevelName(state.feature_level));
    Log("[info] path: " + std::string(state.using_swap_chain
        ? "swapchain + offscreen"
        : "offscreen fallback (no swapchain)"));
    LogAdapter(state.device.Get());

    if (!CheckBgraSupport(state.device.Get())) {
        Log("[fail] RESULT: FAIL (BGRA support)");
        return 1;
    }
    if (!RunOffscreenTriangle(state.device.Get(), state.context.Get())) {
        Log("[fail] RESULT: FAIL (offscreen triangle/readback)");
        return 1;
    }

    if (state.using_swap_chain) {
        if (!PresentClear(
                state.context.Get(), state.swap_chain.Get(),
                state.swap_chain_view.Get())) {
            Log("[fail] RESULT: FAIL (present)");
            return 1;
        }
        Log("[pass] RESULT: PASS (swapchain present + offscreen readback)");
    } else {
        Log("[warn] present: skipped (no swapchain)");
        Log("[pass] RESULT: PASS (offscreen fallback; swapchain unavailable)");
    }
    return 0;
}

}  // namespace

int main() {
    SetConsoleOutputCP(CP_UTF8);
    Log("=== Cinnabar D3D11 probe ===");
    LogModulePath("d3dcompiler_47", L"d3dcompiler_47.dll");

    const HINSTANCE instance = GetModuleHandleW(nullptr);
    HWND window = CreateProbeWindow(instance);
    const int result = RunProbe(window);
    if (window != nullptr) {
        DestroyWindow(window);
    }

    Log("[info] exit code: " + std::to_string(result));
    return result;
}
