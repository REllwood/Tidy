# Existing installations

New installations use Tidy identifiers throughout the application, database, preferences and MCP helper.

The values in `previous-installation.json` are compatibility metadata. They are intentionally retained so an upgrade can find an existing workspace and honour an existing MCP environment configuration. The application reuses that workspace in place; it does not move, overwrite or remove its notes, recordings or downloaded models. A workspace created under the current identifier takes precedence.

Browser preferences are copied to their current keys only when those keys do not already exist. The previous values remain intact.

macOS may request microphone and screen-recording permissions again because the application identifier has changed. Existing MCP client registrations should be updated using the command shown in Tidy Settings, as the bundled helper now uses the Tidy executable name.
