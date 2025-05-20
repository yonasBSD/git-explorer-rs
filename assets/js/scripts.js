document.addEventListener('DOMContentLoaded', async function() {
    let repoName = window.location.pathname.split("/")[4];

    const cacheAvailable = 'caches' in self;
    if (cacheAvailable) {

        // you can safely insert your snippet here
        const newCache = await caches.open('repos');

        // retrieve a new response
        const request = `/api/v1/repo/${repoName}/commits/json`;
        const response = await newCache.match(request);

        newCache.match(request)
        .then((response) => {
            if (response) {
                response.text().then(function(text) {
                  displayCommits(JSON.parse(text));
                });
            } else {
                newCache.add(request)
                .then(function() {
                    newCache.match(request)
                    .then((response) => {
                        response.text().then(function(text) {
                          displayCommits(JSON.parse(text));
                      });
                });
            });
            }
        });
    }

    axios.get(`/api/v1/repo/${repoName}/commits/json`)
        .then(response => {
            displayCommits(response.data);
        })
        .catch(error => {
            console.error('Error fetching commits:', error);
        });
});

function displayCommits(commits) {
    const commitsList = document.getElementById('commits-list');

    const commitsHTML = commits.map(commit => {
        const commitDate = moment.unix(commit.date).fromNow();

        return `
        <!--
            <div class="bg-white shadow p-4 mb-4 rounded">
                <h2 class="text-lg font-bold">${commit.author}</h2>
                <p class="text-gray-600">${commit.message}</p>
                <p class="text-sm text-gray-500">${commitDate}</p>
            </div>
          -->

            <div class="row">
              <div class="col s12 m6">
                <div class="card blue-grey darken-1">
                  <div class="card-content white-text">
                    <span class="card-title">${commit.author}</span>
                    <p>${commit.message}</p>
                  </div>
                  <div class="card-action">
                    <div class="text-sm text-gray-500">${commitDate}</div>
                  </div>
                </div>
              </div>
          </div>
        `;
    }).join('');

    commitsList.innerHTML = commitsHTML;
}
