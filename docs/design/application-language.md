# Application language

The application and screen designs use English.

All owned UI/notification/accessibility strings use the English default resource
set. Romanian resources are removed. The manifest advertises only `en`; Gradle
retains only English language resources, including dependencies. Android documents
this packaging behavior in [ApplicationAndroidResources.localeFilters](https://developer.android.com/reference/tools/gradle-api/8.12/com/android/build/api/dsl/ApplicationAndroidResources#localeFilters()).

The Activity and common service attach an English resource context. The override
sets only locale/layout direction so other device configuration changes remain
available, as specified by [createConfigurationContext](https://developer.android.com/reference/android/content/Context#createConfigurationContext(android.content.res.Configuration)).
Worker resource numbers use Locale.ENGLISH explicitly. No device-wide locale or
guest environment is changed. Package/user data keeps its original contents.

The active configuration form proposal is English throughout. New UI work must
add English strings only. Historical bilingual verification records retain their
tested-revision meaning; current behavior is recorded separately. Compilation
and resource-table inspection do not replace later device/accessibility checks.
