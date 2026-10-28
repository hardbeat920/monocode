const { withEntitlementsPlist } = require("expo/config-plugins");
const withNotifications = require("expo-notifications/app.plugin").default;

module.exports = function withLocalNotifications(config) {
  // Session alerts are scheduled locally; they don't register with APNs.
  // Mods run in reverse registration order, so register this cleanup before
  // the notifications plugin that adds the push entitlement by default.
  config = withEntitlementsPlist(config, (config) => {
    delete config.modResults["aps-environment"];
    return config;
  });

  return withNotifications(config);
};
