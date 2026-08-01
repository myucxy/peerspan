#include <cstdio>

#include <windows.h>
#include <swdevice.h>
#include <conio.h>

struct CreationContext
{
    HANDLE Event;
    HRESULT Result;
};

VOID WINAPI
CreationCallback(
    _In_ HSWDEVICE hSwDevice,
    _In_ HRESULT hrCreateResult,
    _In_opt_ PVOID pContext,
    _In_opt_ PCWSTR pszDeviceInstanceId
    )
{
    auto* context = static_cast<CreationContext*>(pContext);

    context->Result = hrCreateResult;
    SetEvent(context->Event);
    UNREFERENCED_PARAMETER(hSwDevice);
    UNREFERENCED_PARAMETER(hrCreateResult);
    UNREFERENCED_PARAMETER(pszDeviceInstanceId);
}

int __cdecl main(int argc, wchar_t *argv[])
{
    UNREFERENCED_PARAMETER(argc);
    UNREFERENCED_PARAMETER(argv);

    HANDLE hEvent = CreateEvent(nullptr, FALSE, FALSE, nullptr);
    if (hEvent == nullptr)
    {
        printf("CreateEvent failed with %lu\n", GetLastError());
        return 1;
    }

    CreationContext context = { hEvent, E_PENDING };
    HSWDEVICE hSwDevice = nullptr;
    SW_DEVICE_CREATE_INFO createInfo = { 0 };
    PCWSTR description = L"PeerSpan Virtual Display";

    // These match the Pnp id's in the inf file so OS will load the driver when the device is created
    PCWSTR instanceId = L"PeerSpanVirtualDisplay";
    PCWSTR hardwareIds = L"PeerSpanVirtualDisplay\0\0";
    PCWSTR compatibleIds = L"PeerSpanVirtualDisplay\0\0";

    createInfo.cbSize = sizeof(createInfo);
    createInfo.pszzCompatibleIds = compatibleIds;
    createInfo.pszInstanceId = instanceId;
    createInfo.pszzHardwareIds = hardwareIds;
    createInfo.pszDeviceDescription = description;

    createInfo.CapabilityFlags = SWDeviceCapabilitiesRemovable |
                                 SWDeviceCapabilitiesSilentInstall |
                                 SWDeviceCapabilitiesDriverRequired;

    // Create the device
    HRESULT hr = SwDeviceCreate(L"PeerSpanVirtualDisplay",
                                L"HTREE\\ROOT\\0",
                                &createInfo,
                                0,
                                nullptr,
                                CreationCallback,
                                &context,
                                &hSwDevice);
    if (FAILED(hr))
    {
        printf("SwDeviceCreate failed with 0x%lx\n", hr);
        CloseHandle(hEvent);
        return 1;
    }

    // Wait for callback to signal that the device has been created
    printf("Waiting for device to be created....\n");
    DWORD waitResult = WaitForSingleObject(hEvent, 10*1000);
    if (waitResult != WAIT_OBJECT_0)
    {
        printf("Wait for device creation failed\n");
        SwDeviceClose(hSwDevice);
        CloseHandle(hEvent);
        return 1;
    }
    if (FAILED(context.Result))
    {
        printf("Software device creation completed with 0x%lx\n", context.Result);
        SwDeviceClose(hSwDevice);
        CloseHandle(hEvent);
        return 1;
    }
    printf("PeerSpan virtual display created\n\n");

    // Now wait for user to indicate the device should be stopped
    printf("Press 'x' to exit and destroy the software device\n");
    bool bExit = false;
    do
    {
        // Wait for key press
        int key = _getch();

        if (key == 'x' || key == 'X')
        {
            bExit = true;
        }
    }while (!bExit);

    // Stop the software device, which unloads the prototype driver instance.
    SwDeviceClose(hSwDevice);
    CloseHandle(hEvent);

    return 0;
}
