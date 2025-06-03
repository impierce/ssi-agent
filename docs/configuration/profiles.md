# Application Profiles

UniCore can run with different application profiles. An application profile includes configuration defaults, changes runtime behavior and can enforce certain restrictions.
Currently, UniCore supports the following application profiles:

### Production _(default)_

If nothing is further specified, UniCore will run in production mode by default.

### Development

Can be enabled by setting the environment variable `UNICORE__PROFILE=development`.

This profile is designed to be used for development purposes. It requires less initial configuration, enables a more verbose API and debugging information.
