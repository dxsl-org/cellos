var first = document.getElementById('first');
first.textContent = 'Script updated the real document';
var created = document.createElement('p');
created.textContent = 'Created by JavaScript';
document.body.appendChild(created);
document.getElementById('change').addEventListener('click', function () {
    document.getElementById('result').textContent = 'Click updated the document';
});
