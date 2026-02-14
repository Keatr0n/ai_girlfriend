import requests


def get_current_time() -> str:
    """Get current time in specified timezone"""
    from datetime import datetime

    return datetime.now().strftime("%Y-%m-%d %H:%M:%S %Z")


def get_weather(city: str) -> dict:
    """Get current weather for a location"""
    url = "https://nominatim.openstreetmap.org/search"
    params = {
        "q": city,
        "format": "json",
        "limit": 1
    }
    headers = {
        "User-Agent": "ai-girlfriend-tools/1.0"
    }

    response = requests.get(url, params=params, headers=headers)
    response.raise_for_status()
    data = response.json()

    latitude = float(data[0]["lat"])
    longitude = float(data[0]["lon"])

    url = f"https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}&hourly=temperature_2m,precipitation_probability"

    response = requests.get(url)
    response.raise_for_status()

    data = response.json()

    # Get current hour's data (first entry)
    current_temp = data["hourly"]["temperature_2m"][0]
    current_precip = data["hourly"]["precipitation_probability"][0]
    current_time = data["hourly"]["time"][0]

    return {
        "location": f"{latitude}, {longitude}",
        "temperature": current_temp,
        "temperature_unit": data["hourly_units"]["temperature_2m"],
        "precipitation_probability": current_precip,
        "time": current_time,
        "timezone": data["timezone"]
    }
