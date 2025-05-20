document.addEventListener('DOMContentLoaded', async function() {
    let repoName = window.location.pathname.split("/")[2];

    const cacheAvailable = 'caches' in self;
    if (cacheAvailable) {

        // you can safely insert your snippet here
        const newCache = await caches.open('repos');

        // retrieve a new response
        const request = `/repo/${repoName}/commits/json`;
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

    /*
    axios.get(`/repo/${repoName}/commits/json`)
        .then(response => {
            displayCommits(response.data);
        })
        .catch(error => {
            console.error('Error fetching commits:', error);
        });
  */
});

function displayCommits(commits) {
    const commitsList = document.getElementById('commits-list');

    const commitsHTML = commits.map(commit => {
        const commitDate = moment.unix(commit.date).fromNow();

        return `
            <div class="bg-white shadow p-4 mb-4 rounded">
                <h2 class="text-lg font-bold">${commit.author}</h2>
                <p class="text-gray-600">${commit.message}</p>
                <p class="text-sm text-gray-500">${commitDate}</p>
            </div>
        `;
    }).join('');

    commitsList.innerHTML = commitsHTML;
}
