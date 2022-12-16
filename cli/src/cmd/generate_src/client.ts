/**
 * Container for extra configuration information for client API requests.
 */
export type ClientConfig = Ωlib.ClientConfig;

/**
 * Creates an object that exposes an API to make requests of a ChiselStrike
 * backend using the automatically generated [RESTful entity CRUD
 * API](https://docs.chiselstrike.com/reference/entity-crud-api/). This API
 * invokes the [entity CRUD
 * routes](https://docs.chiselstrike.com/reference/routing/entity-crud) declared
 * in the backend.
 *
 * **Creating a client object**
 *
 * To create a client configured to access a ChiselStrike backend running
 * locally during development using all the defaults (localhost port 8080,
 * version "dev"):
 * ```
 * const chiselClient = createChiselClient("http://localhost:8080")
 * ```
 *
 * To create a client configured to access a ChiselStrike backend running using
 * the ChiselStrike managed service:
 *
 * ```
 * const baseUrl = "https://[REPO-NAME]-[GITHUB-ACCOUNT].chiselstrike.io"
 * const config = { version: "[BRANCH]" }
 * const chiselClient = createChiselClient(baseUrl, config)
 * ```
 *
 * Where `[REPO-NAME]` and `[GITHUB-ACCOUNT]` are the names of your GitHub repo
 * and user name, and `[BRANCH]` is the name of your production branch.
 *
 * The returned object contains one property for each entity CRUD route declared
 * in the backend. The name of the property is the same as the name of the
 * route. Each of these properties derived from an entity route has methods that
 * operate on instances of that entity and are generically typed to accept and
 * return generated class instances with the same name as the entity.
 *
 * **Generated entity API methods**
 *
 * For an entity "MyEntity" with a CRUD route "myEntities", the generated
 * methods that operate on zero or more entity instances are:
 *
 * - `chiselClient.myEntities.delete()`
 * - `chiselClient.myEntities.get()`
 * - `chiselClient.myEntities.getIter()`
 * - `chiselClient.myEntities.post()`
 *
 * The generated methods that deal with specific instances of an entity given
 * its unique ID are:
 *
 * - `chiselClient.myEntities.id(id).delete()`
 * - `chiselClient.myEntities.id(id).get()`
 * - `chiselClient.myEntities.id(id).patch()`
 * - `chiselClient.myEntities.id(id).put()`
 *
 * Each of these generated methods corresponds to the functionality of the
 * [ChiselStrike RESTful entity CRUD HTTP
 * methods](https://docs.chiselstrike.com/reference/entity-crud-api/supported-http-methods).
 * Each call returns a promise that becomes fulfilled or rejected upon
 * completion, except for `getIter()` which returns an AsyncIterable`.
 *
 * @param serverUrl - The base endpoint URL of the backend service
 * @param config - ClientConfig object that enables customization of the
 *   requests issues by the client. The default client is configured to use the
 *   version of the backend for which the API was generated.
 * @returns an object configured to make requests of ChiselStrike entities
 */
export function createChiselClient(
    config: ClientConfig,
) {
    return ΩcreateClient(config);
}
