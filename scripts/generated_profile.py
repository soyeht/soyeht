# Generated from contracts/profiles.json. Do not edit.
PROFILES = {
    "dev": {
        "dataDirectory": {
            "macos": "Soyeht.next/dev",
            "windows": "Soyeht.next/dev",
            "linux": "soyeht-next/dev"
        },
        "tcpPort": 8291,
        "appIds": {
            "macos": "com.soyeht.next.dev.mac",
            "linux": "com.soyeht.next.dev"
        },
        "launchdLabels": {
            "engine": "com.soyeht.next.dev.engine",
            "ptyd": "com.soyeht.next.dev.ptyd"
        },
        "launchdPrefix": "com.soyeht.next.dev"
    },
    "prod": {
        "dataDirectory": {
            "macos": "Soyeht.next",
            "windows": "Soyeht.next",
            "linux": "soyeht-next"
        },
        "tcpPort": 8290,
        "appIds": {
            "macos": "com.soyeht.next.mac",
            "linux": "com.soyeht.next"
        },
        "launchdLabels": {
            "engine": "com.soyeht.next.engine",
            "ptyd": "com.soyeht.next.ptyd"
        },
        "launchdPrefix": "com.soyeht.next"
    }
}
SELECTION_ENVIRONMENT = 'SOYEHT_PROFILE'
BUILD_ENVIRONMENT = 'SOYEHT_BUILD_PROFILE'
ENGINE_SOCKET = 'engine.sock'
PTYD_SOCKET = 'ptyd.sock'
